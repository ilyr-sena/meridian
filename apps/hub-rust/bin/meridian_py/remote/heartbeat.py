"""
Heartbeat reporter for Meridian devices.
Pushes real-time device presence, status (online/offline), and network endpoints
to the central Meridian Next.js API / MongoDB.
"""
from __future__ import annotations

import json
import logging
import os
import sys
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Dict, Optional

from .mesh import get_mesh_ip

log = logging.getLogger(__name__)


def get_api_url() -> str:
    return os.environ.get("MERIDIAN_API_URL", "http://localhost:3000").rstrip("/")


def _get_device_meta(udid: str) -> dict:
    meta = {
        "name": "iPhone",
        "model": "iPhone",
        "version": "iOS 27.0",
    }
    if sys.platform == "win32":
        appdata = os.environ.get("LOCALAPPDATA") or str(Path.home() / "AppData" / "Local")
        dev_file = Path(appdata) / "Meridian" / "state" / "devices.json"
    else:
        dev_file = Path.home() / ".local" / "state" / "meridian" / "devices.json"
    if dev_file.exists():
        try:
            data = json.loads(dev_file.read_text())
            if udid in data:
                entry = data[udid]
                meta["name"] = entry.get("name") or meta["name"]
                m = entry.get("model", "")
                if "iPhone14,5" in m:
                    meta["model"] = "iPhone 13"
                elif m:
                    meta["model"] = m
                meta["version"] = f"iOS {entry.get('ios_version', '27.0')}"
        except Exception:
            pass
    return meta


def send_device_heartbeat(
    udid: str,
    status: str = "online",
    host_ports: Optional[dict] = None,
    meta: Optional[dict] = None,
    api_url: Optional[str] = None,
    timeout: float = 4.0,
) -> bool:
    url = f"{api_url or get_api_url()}/api/devices/heartbeat"
    device_meta = meta or _get_device_meta(udid)
    mesh_ip = get_mesh_ip()

    ports = host_ports or {
        "wda": 8100,
        "bridge": 9001,
        "stream": 9200,
    }

    payload = {
        "udid": udid,
        "name": device_meta.get("name", "iPhone"),
        "model": device_meta.get("model", "iPhone"),
        "version": device_meta.get("version", "iOS 27.0"),
        "status": status,
        "tailscale_ip": mesh_ip,
        "host_ports": ports,
    }

    try:
        data = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(
            url,
            data=data,
            headers={
                "Content-Type": "application/json",
                "User-Agent": "MeridianDaemon/0.2.0",
            },
            method="POST",
        )
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            if resp.status in (200, 201):
                return True
    except urllib.error.URLError as e:
        log.debug("heartbeat ping failed (%s): %s", url, e)
    except Exception as e:
        log.debug("heartbeat error: %s", e)
    return False


class DeviceHeartbeatWorker:
    """Independent background heartbeat loop for a single device."""

    def __init__(
        self,
        udid: str,
        host_ports: Optional[dict] = None,
        meta: Optional[dict] = None,
        api_url: Optional[str] = None,
        interval: float = 10.0,
    ):
        self.udid = udid
        self.host_ports = host_ports
        self.meta = meta
        self.api_url = api_url
        self.interval = interval
        self._stop = threading.Event()
        self._thread: Optional[threading.Thread] = None

    def start(self) -> None:
        self._stop.clear()
        self._thread = threading.Thread(target=self._run, daemon=True, name=f"heartbeat-{self.udid[-6:]}")
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=2.0)
        # Final offline push
        send_device_heartbeat(self.udid, status="offline", host_ports=self.host_ports, meta=self.meta, api_url=self.api_url)

    def _run(self) -> None:
        from ..mux.client import MuxClient, DEFAULT_MUX_ENDPOINT
        log.info("heartbeat worker active for %s (ports: %s)", self.udid, self.host_ports)
        # Initial heartbeat
        time.sleep(1.0)
        send_device_heartbeat(self.udid, status="online", host_ports=self.host_ports, meta=self.meta, api_url=self.api_url)

        while not self._stop.is_set():
            self._stop.wait(self.interval)
            if self._stop.is_set():
                break

            # Verify physical presence via usbmuxd
            attached = []
            try:
                mux = MuxClient(DEFAULT_MUX_ENDPOINT)
                attached = [d.udid for d in mux.list_devices()]
            except Exception:
                attached = []

            if self.udid not in attached:
                send_device_heartbeat(self.udid, status="offline", host_ports=self.host_ports, meta=self.meta, api_url=self.api_url)
                continue

            send_device_heartbeat(self.udid, status="online", host_ports=self.host_ports, meta=self.meta, api_url=self.api_url)


# Global multi-device worker registry
_WORKERS: Dict[str, DeviceHeartbeatWorker] = {}
_WORKERS_LOCK = threading.Lock()


def start_heartbeat_worker(
    udid: str,
    host_ports: Optional[dict] = None,
    meta: Optional[dict] = None,
    api_url: Optional[str] = None,
    interval: float = 10.0,
):
    with _WORKERS_LOCK:
        if udid in _WORKERS:
            _WORKERS[udid].stop()
        worker = DeviceHeartbeatWorker(
            udid=udid,
            host_ports=host_ports,
            meta=meta,
            api_url=api_url,
            interval=interval,
        )
        worker.start()
        _WORKERS[udid] = worker


def stop_heartbeat_worker(udid: Optional[str] = None):
    with _WORKERS_LOCK:
        if udid:
            worker = _WORKERS.pop(udid, None)
            if worker:
                worker.stop()
        else:
            for w in list(_WORKERS.values()):
                w.stop()
            _WORKERS.clear()
