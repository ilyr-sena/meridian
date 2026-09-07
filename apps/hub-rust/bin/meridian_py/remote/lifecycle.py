"""
Device Lifecycle Manager for Meridian Remote.
Handles USB hotplug attach/detach, automatic runner launch, tunnels,
and real-time MongoDB/SSE presence synchronization.
"""
from __future__ import annotations

import asyncio
import json
import logging
import threading
import time
import urllib.request
import urllib.error
from typing import Optional, Set

from ..devices.watcher import Watcher
from ..mux.client import MuxClient, DEFAULT_MUX_ENDPOINT
from ..mux.tunnel import Tunnel
from .heartbeat import send_device_heartbeat, start_heartbeat_worker
from .bridge import _rsd_for_device, RSD_CACHE, reset_gesture_worker
from pymobiledevice3.remote.core_device.app_service import AppServiceService

log = logging.getLogger(__name__)

RUNNER_BUNDLE_ID = "dev.ius.meridian.runner.xctrunner.SRTHYBYH35"


class DeviceLifecycleManager:
    def __init__(self, target_udid: Optional[str] = None, api_url: str = ""):
        self.target_udid = target_udid
        self.api_url = api_url
        self.mux = MuxClient(DEFAULT_MUX_ENDPOINT)
        self.watcher = Watcher(self.mux)
        self.tunnels: list[Tunnel] = []
        self._stopping = threading.Event()
        self._action_lock = threading.Lock()
        self._active_udids: Set[str] = set()

    def start(self):
        log.info("starting device lifecycle manager (target: %s)...", self.target_udid or "auto")
        self._ensure_tunnels()
        self.watcher.add_listener(self._on_device_event)
        self.watcher.start()

        # Check currently attached devices on startup
        attached = self._get_attached_udids()
        if attached:
            chosen = self.target_udid if self.target_udid in attached else attached[0]
            threading.Thread(target=self._handle_attach, args=(chosen,), daemon=True).start()
        else:
            log.warning("no iOS devices currently attached over USB; waiting for connection...")

    def stop(self):
        self._stopping.set()
        self.watcher.stop()
        self._stop_tunnels()
        for udid in list(self._active_udids):
            send_device_heartbeat(udid, status="offline", api_url=self.api_url)
        self._active_udids.clear()

    def _get_attached_udids(self) -> list[str]:
        try:
            return [d.udid for d in self.mux.list_devices()]
        except Exception:
            return []

    def _stop_tunnels(self):
        for t in self.tunnels:
            try:
                t.stop()
            except Exception:
                pass
        self.tunnels.clear()

    def _ensure_tunnels(self):
        if self.tunnels:
            return
        pairs = [(8100, 8100), (9200, 9200)]
        for lp, dp in pairs:
            try:
                t = Tunnel(self.mux, lp, dp, self.target_udid)
                t.start()
                self.tunnels.append(t)
                log.info("✓ tunnel established: localhost:%d -> device:%d", lp, dp)
            except Exception as e:
                log.warning("could not bind tunnel :%d -> :%d: %s", lp, dp, e)

    def _is_wda_alive(self) -> bool:
        try:
            req = urllib.request.Request("http://127.0.0.1:8100/status")
            with urllib.request.urlopen(req, timeout=1.5) as resp:
                if resp.status == 200:
                    data = json.loads(resp.read() or b"{}")
                    return data.get("value", {}).get("state") == "success"
        except Exception:
            pass
        return False

    def _on_device_event(self, kind: str, payload: dict):
        if kind == "detach_all":
            for u in list(self._active_udids):
                self._handle_detach(u)
            return

        udid = payload.get("udid")
        if not udid:
            return
        if self.target_udid and udid != self.target_udid:
            return

        if kind == "detach":
            self._handle_detach(udid)
        elif kind == "attach":
            threading.Thread(target=self._handle_attach, args=(udid,), daemon=True).start()

    def _handle_detach(self, udid: str):
        with self._action_lock:
            log.info("🔌 Device detached from USB: %s", udid)
            self._active_udids.discard(udid)
            reset_gesture_worker()

            # Immediate status update to central API
            send_device_heartbeat(udid, status="offline", api_url=self.api_url)
            log.info("✓ Reported device %s as OFFLINE to database", udid)

    def _handle_attach(self, udid: str):
        with self._action_lock:
            if self._stopping.is_set():
                return
            if udid in self._active_udids and self._is_wda_alive():
                return  # already running and healthy

            log.info("⚡ Device attached over USB: %s", udid)
            self._active_udids.add(udid)
            RSD_CACHE["udid"] = udid
            reset_gesture_worker()

            # Ensure tunnels on 8100 and 9200 are active
            self._ensure_tunnels()

            # Check if MeridianRunner is ALREADY alive on device
            if self._is_wda_alive():
                log.info("✓ MeridianRunner is already running on device %s", udid)
                send_device_heartbeat(udid, status="online", api_url=self.api_url)
                start_heartbeat_worker(udid, api_url=self.api_url)
                return

            # Otherwise, launch MeridianRunner
            self._prepare_device_and_launch(udid)

    def _prepare_device_and_launch(self, udid: str):
        log.info("launching MeridianRunner on %s...", udid)
        async def _launch():
            try:
                rsd = await _rsd_for_device(udid)
                async with AppServiceService(rsd) as app_service:
                    await app_service.launch_application(RUNNER_BUNDLE_ID)
                    log.info("✓ MeridianRunner launched successfully on device")
            except Exception as e:
                log.warning("failed to launch MeridianRunner: %s", e)

        try:
            asyncio.run(_launch())
        except Exception as e:
            log.warning("error in async runner launch: %s", e)

        # Wait briefly for server startup and prompt capture start
        time.sleep(1.0)
        self._ensure_capture_started()

        # Report device as online with Tailscale IP
        send_device_heartbeat(udid, status="online", api_url=self.api_url)
        log.info("🚀 Device %s online and ready!", udid)

    def _ensure_capture_started(self):
        try:
            req = urllib.request.Request("http://127.0.0.1:9200/capture/start", data=b"", method="POST")
            with urllib.request.urlopen(req, timeout=2.0) as resp:
                pass
        except Exception:
            pass
