"""
Multi-Device Orchestrator for Meridian.

Manages multiple connected iPhones simultaneously on a single host machine:
  - Dynamically assigns isolated port blocks (Slot 0, Slot 1, ...) in connection order.
  - Runs the deterministic Onboarding State Machine (Trust, Passcode, iOS 27, AMFI DevMode, Runner Sideload).
  - Isolates tunnels, 60Hz CoreDevice HID digitizers, and WebSockets per device.
  - Keeps MongoDB Atlas device presence in sync (status: online | offline).
"""
from __future__ import annotations

import asyncio
import logging
import subprocess
import sys
import threading
import time
from dataclasses import asdict
from typing import Callable, Dict, List, Optional

from ..core.inspector import DeviceInspector, DeviceReport, DeviceState
from ..core.slots import GLOBAL_SLOTS, DevicePorts
from ..devices.watcher import Watcher
from ..mux.client import MuxClient, DEFAULT_MUX_ENDPOINT
from ..mux.tunnel import Tunnel
from .bridge import BridgeServer, BridgeHandler, RS, ACTION_CTX, gesture_worker, reset_gesture_worker
from .heartbeat import DeviceHeartbeatWorker, send_device_heartbeat
from .mesh import get_mesh_ip, start_mesh, stop_mesh

log = logging.getLogger(__name__)

RUNNER_BUNDLE_ID = "dev.ius.meridian.runner.xctrunner.SRTHYBYH35"


class DeviceWorker:
    """Manages all active runtime services for a single connected iPhone."""

    def __init__(self, udid: str, ports: DevicePorts, report: DeviceReport, api_url: str = ""):
        self.udid = udid
        self.ports = ports
        self.report = report
        self.api_url = api_url
        self.tunnels: List[Tunnel] = []
        self.heartbeat: Optional[DeviceHeartbeatWorker] = None
        self.bridge: Optional[BridgeServer] = None
        self.is_running = False
        self._stop_event = threading.Event()

    def start(self) -> None:
        """Start all tunnels, launch runner, arm HID touch, and start heartbeat."""
        if self.is_running:
            return
        log.info("🚀 Starting Meridian services for %s on %s...", self.udid, self.ports)
        self._stop_event.clear()

        # 1. Establish persistent usbmuxd tunnels
        mux = MuxClient(DEFAULT_MUX_ENDPOINT)
        for lp, dp in [(self.ports.wda, 8100), (self.ports.stream, 9200)]:
            try:
                t = Tunnel(mux, lp, dp, self.udid)
                t.start()
                self.tunnels.append(t)
                log.info("✓ Device %s tunnel established: localhost:%d -> device:%d", self.udid, lp, dp)
            except Exception as e:
                log.warning("could not bind tunnel %d->%d for %s: %s", lp, dp, self.udid, e)

        # 2. Launch MeridianRunner if installed
        self._launch_runner()

        # 3. Start CoreDevice HID touch and control bridge on assigned port
        self.bridge = BridgeServer(host="127.0.0.1", port=self.ports.bridge, udid=self.udid)
        self.bridge.start()

        # 4. Start Heartbeat worker to sync MongoDB
        meta = {
            "name": self.report.name,
            "model": self.report.model,
            "version": self.report.os_version,
        }
        self.heartbeat = DeviceHeartbeatWorker(
            udid=self.udid,
            host_ports=self.ports.to_dict(),
            meta=meta,
            api_url=self.api_url,
        )
        self.heartbeat.start()

        self.is_running = True
        self.report.state = DeviceState.RUNNING
        self.report.status_message = f"Streaming on port {self.ports.stream}"

    def stop(self) -> None:
        """Stop all runtime services for this device."""
        if not self.is_running:
            return
        log.info("⏹ Stopping Meridian services for %s...", self.udid)
        self._stop_event.set()

        # 1. Kill the opened MeridianRunner app on the iPhone
        self._kill_runner()

        # 2. Stop bridge
        if self.bridge:
            self.bridge.stop()
            self.bridge = None

        # 3. Stop heartbeat (sends final offline status)
        if self.heartbeat:
            self.heartbeat.stop()
            self.heartbeat = None

        # 4. Stop tunnels
        for t in self.tunnels:
            try: t.stop()
            except Exception: pass
        self.tunnels.clear()

        self.is_running = False
        self.report.state = DeviceState.READY
        self.report.status_message = "Stopped"

    def _kill_runner(self) -> None:
        """Terminate the running MeridianRunner process on the iPhone."""
        from pymobiledevice3.remote.core_device.app_service import AppServiceService
        from .bridge import _rsd_for_device

        async def _kill():
            try:
                rsd = await _rsd_for_device(self.udid)
                async with AppServiceService(rsd) as app_service:
                    procs = await app_service.list_processes()
                    for p in procs:
                        url = str(p.get("executableURL", {}).get("relative", ""))
                        pid = p.get("processIdentifier")
                        if pid and ("runner" in url.lower() or "meridian" in url.lower() or "webdriver" in url.lower()):
                            try:
                                await app_service.send_signal_to_process(pid, 9)
                                log.info("✓ Terminated MeridianRunner (PID %s) on iPhone %s", pid, self.udid)
                            except Exception:
                                pass
            except Exception as e:
                log.debug("runner kill error on %s: %s", self.udid, e)

        try:
            asyncio.run(_kill())
        except Exception as e:
            log.debug("runner termination failed for %s: %s", self.udid, e)

    def _launch_runner(self) -> None:
        """Ensure MeridianRunner is launched on device, with retries."""
        from pymobiledevice3.remote.core_device.app_service import AppServiceService
        from .bridge import _rsd_for_device

        bid = self.report.runner_bundle_id or RUNNER_BUNDLE_ID

        def _launch_with_retries():
            for attempt in range(4):
                try:
                    async def _do():
                        rsd = await _rsd_for_device(self.udid)
                        async with AppServiceService(rsd) as app_service:
                            await app_service.launch_application(bid)
                    asyncio.run(_do())
                    log.info("✓ MeridianRunner launched successfully on %s", self.udid)
                    return
                except Exception as e:
                    if attempt < 3:
                        log.info("runner launch attempt %d failed, retrying in %ds: %s", attempt + 1, 2 * (attempt + 1), e)
                        time.sleep(2 * (attempt + 1))
                    else:
                        log.warning("runner launch failed after %d attempts: %s", attempt + 1, e)

        threading.Thread(target=_launch_with_retries, daemon=True).start()


class MultiDeviceOrchestrator:
    """Coordinates detection, onboarding, port assignment, and workers for all attached iPhones."""

    def __init__(self, on_change: Optional[Callable[[DeviceReport], None]] = None, api_url: str = ""):
        self.on_change = on_change
        self.api_url = api_url
        self._workers: Dict[str, DeviceWorker] = {}
        self._reports: Dict[str, DeviceReport] = {}
        self._inspecting_udids = set()
        self._lock = threading.Lock()
        self._watcher: Optional[Watcher] = None
        self._auto_start = True
        self._poll_stop = threading.Event()
        self._poll_thread: Optional[threading.Thread] = None
        self._tunneld_proc: Optional[subprocess.Popen] = None

    def start(self, auto_start_ready: bool = True) -> None:
        """Start the orchestrator, Tailscale mesh, and usbmuxd watcher."""
        self._auto_start = auto_start_ready
        log.info("starting Meridian Multi-Device Orchestrator...")

        # 1. Start Tailscale mesh sidecar
        start_mesh()

        # 2. Start pymobiledevice3 tunneld if not already listening
        self._ensure_tunneld()

        # 3. Start USB hotplug watcher
        mux = MuxClient(DEFAULT_MUX_ENDPOINT)
        self._watcher = Watcher(mux)
        self._watcher.add_listener(self._on_usb_event)
        self._watcher.start()

        # 4. Start background poller for pending onboarding devices (auto-detects trust, unlocks, and sideloads)
        self._poll_stop.clear()
        self._poll_thread = threading.Thread(target=self._poll_pending_loop, daemon=True, name="orch-pending-poll")
        self._poll_thread.start()

        # Initial scan of already-attached devices
        self._initial_scan()

    def stop(self) -> None:
        """Stop all workers, tunnels, and watcher."""
        log.info("stopping Multi-Device Orchestrator...")
        self._poll_stop.set()
        if self._watcher:
            self._watcher.stop()
            self._watcher = None

        with self._lock:
            for w in list(self._workers.values()):
                w.stop()
            self._workers.clear()

        stop_mesh()

        if self._tunneld_proc:
            try:
                self._tunneld_proc.terminate()
            except Exception:
                pass
            self._tunneld_proc = None

    def _ensure_tunneld(self) -> None:
        """Start pymobiledevice3 tunneld if not already listening."""
        import socket as _sock
        with _sock.socket(_sock.AF_INET, _sock.SOCK_STREAM) as s:
            s.settimeout(0.5)
            if s.connect_ex(("127.0.0.1", 49151)) == 0:
                log.info("tunneld already listening on :49151")
                return
        log.info("starting pymobiledevice3 tunneld on :49151...")
        if sys.platform == "win32":
            try:
                import ctypes
                ctypes.windll.shell32.ShellExecuteW(
                    None, "runas", sys.executable,
                    "-m pymobiledevice3 remote tunneld",
                    None, 0,
                )
            except Exception as e:
                log.warning("failed to start tunneld with elevation: %s", e)
                return
        else:
            _NO_WINDOW = 0x08000000 if sys.platform == "win32" else 0
            proc = subprocess.Popen(
                [sys.executable, "-m", "pymobiledevice3", "remote", "tunneld"],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                creationflags=_NO_WINDOW,
            )
            self._tunneld_proc = proc
        for _ in range(30):
            time.sleep(0.5)
            with _sock.socket(_sock.AF_INET, _sock.SOCK_STREAM) as s:
                s.settimeout(0.5)
                if s.connect_ex(("127.0.0.1", 49151)) == 0:
                    log.info("tunneld ready on :49151")
                    return
        log.warning("tunneld did not come up in 15s — HID/runner features unavailable (run tunneld as admin first)")

    def _initial_scan(self) -> None:
        def _scan():
            time.sleep(0.5)
            try:
                mux = MuxClient(DEFAULT_MUX_ENDPOINT)
                devices = mux.list_devices()
                for d in devices:
                    self._handle_attach(d.udid)
            except Exception as e:
                log.debug("initial scan error: %s", e)
        threading.Thread(target=_scan, daemon=True, name="orch-init-scan").start()

    def _on_usb_event(self, kind: str, payload: dict) -> None:
        if kind == "detach_all":
            with self._lock:
                for udid in list(self._reports.keys()):
                    self._handle_detach(udid)
            return

        udid = payload.get("udid")
        if not udid:
            return

        if kind == "attach":
            threading.Thread(target=self._handle_attach, args=(udid,), daemon=True, name=f"attach-{udid[-6:]}").start()
        elif kind == "detach":
            self._handle_detach(udid)

    def _handle_attach(self, udid: str) -> None:
        with self._lock:
            if udid in self._inspecting_udids:
                return
            self._inspecting_udids.add(udid)
            # 1. Allocate port slot in exact connection order
            ports = GLOBAL_SLOTS.acquire(udid)

        try:
            # 2. Run onboarding inspection
            report = asyncio.run(DeviceInspector.inspect(udid))

            with self._lock:
                self._reports[udid] = report
                worker = DeviceWorker(udid=udid, ports=ports, report=report, api_url=self.api_url)
                self._workers[udid] = worker

            log.info("📱 Device %s connected -> Slot #%d (%s) Status: %s", udid, ports.slot, report.name, report.state.value)

            # Notify UI / listeners
            self._notify_change(report)

            # 3. If ready and auto-start enabled, launch runtime services
            if self._auto_start and report.state == DeviceState.READY:
                worker.start()
                self._notify_change(report)
        finally:
            with self._lock:
                self._inspecting_udids.discard(udid)

    def _handle_detach(self, udid: str) -> None:
        with self._lock:
            worker = self._workers.pop(udid, None)
            report = self._reports.pop(udid, None)
            GLOBAL_SLOTS.release(udid)

        if worker:
            worker.stop()

        if report:
            report.state = DeviceState.DISCONNECTED
            report.status_message = "Disconnected from USB"
            self._notify_change(report)

        # Immediate DB update to mark device offline
        send_device_heartbeat(udid, status="offline", api_url=self.api_url)
        log.info("🔌 Device %s detached and marked offline", udid)

    def _notify_change(self, report: DeviceReport) -> None:
        if self.on_change:
            try: self.on_change(report)
            except Exception as e: log.debug("listener error: %s", e)

    # Public control methods
    def get_report(self, udid: str) -> Optional[DeviceReport]:
        with self._lock:
            return self._reports.get(udid)

    def list_reports(self) -> List[DeviceReport]:
        with self._lock:
            return list(self._reports.values())

    def start_device(self, udid: str) -> bool:
        with self._lock:
            worker = self._workers.get(udid)
        if worker and worker.report.state in (DeviceState.READY, DeviceState.RUNNING):
            worker.start()
            self._notify_change(worker.report)
            return True
        return False

    def stop_device(self, udid: str) -> bool:
        with self._lock:
            worker = self._workers.get(udid)
        if worker:
            worker.stop()
            self._notify_change(worker.report)
            return True
        return False

    def start_all_ready(self) -> int:
        count = 0
        with self._lock:
            workers = list(self._workers.values())
        for w in workers:
            if w.report.state == DeviceState.READY:
                w.start()
                self._notify_change(w.report)
                count += 1
        return count

    def stop_all(self) -> None:
        with self._lock:
            workers = list(self._workers.values())
        for w in workers:
            w.stop()
            self._notify_change(w.report)

    def reinspect_device(self, udid: str) -> Optional[DeviceReport]:
        """Force a fresh re-inspection of the device, update status, and auto-start if ready."""
        try:
            report = asyncio.run(DeviceInspector.inspect(udid))
        except Exception as e:
            log.debug("reinspection error for %s: %s", udid, e)
            return None

        with self._lock:
            self._reports[udid] = report
            worker = self._workers.get(udid)
            if worker:
                worker.report = report

        log.info("📱 Re-inspected %s -> State: %s (%s)", udid, report.state.value, report.status_message)
        self._notify_change(report)

        # Seamless auto-start: If setup just finished and it is ready, start services immediately with no stops!
        if self._auto_start and report.state == DeviceState.READY and worker and not worker.is_running:
            log.info("🚀 Device %s became READY after setup: auto-starting services...", udid)
            worker.start()
            self._notify_change(report)

        return report

    def _poll_pending_loop(self):
        """Continuously re-inspects non-ready devices in background to auto-detect unlocks, trust, and sideloads."""
        while not self._poll_stop.is_set():
            self._poll_stop.wait(3.0)
            if self._poll_stop.is_set():
                break

            with self._lock:
                pending_udids = [
                    u for u, r in self._reports.items()
                    if r.state not in (DeviceState.READY, DeviceState.RUNNING, DeviceState.UNSUPPORTED_IOS)
                ]

            for u in pending_udids:
                try:
                    self.reinspect_device(u)
                except Exception:
                    pass
