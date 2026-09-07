"""CoreDevice 60Hz HID touch digitizer, hardware buttons, app and icon bridge.

Exposes:
- HTTP :9001
    GET /apps.json
    GET /apps/running.json
    GET /icon/<bundleId>.png
    POST /app/launch/<bundleId>
    POST /app/kill/<pid>
- WebSocket ws://localhost:9001/ws
    {kind: "down", fx, fy}
    {kind: "move", fx, fy}
    {kind: "release", fx, fy}
    {kind: "action", name: "home"|"lock"|"volume-up"|"volume-down"}
    {kind: "keyboard", on: bool}
    {kind: "hid", code: str, shift: bool, ctrl: bool, alt: bool}
"""

from __future__ import annotations

import asyncio
import base64
import hashlib
import http.server
import json
import logging
import os
import pathlib
import random
import socket
import sys
import threading
import time
import urllib.parse
from typing import Any, Optional

import requests
from pymobiledevice3.remote.core_device.app_service import AppServiceService
from pymobiledevice3.remote.core_device.hid_service import (
    ASCII_TO_HID,
    HID_BUTTON_STATE_DOWN,
    HID_BUTTON_STATE_UP,
    IndigoHIDService,
    KEY_BACKSPACE,
    KEYBOARD_SURFACE_DEFAULT_SERVICE_ID,
    KEY_ENTER,
    KEY_LEFT_SHIFT,
    KEY_LEFT_ALT,
    KEY_LEFT_CTRL,
    KEY_LEFT_GUI,
    TOUCHSCREEN_STATE_CONTACT,
    TOUCHSCREEN_STATE_RELEASE,
    touch_session,
)
from pymobiledevice3.remote.core_device.icon_service import IconService
from pymobiledevice3.tunneld.api import get_tunneld_devices

log = logging.getLogger(__name__)

WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
STREAM_HZ = 60

NAMED_BUTTONS = {
    "home": (0x0C, 0x40, 0.05),
    "lock": (0x0C, 0x30, 0.5),
    "volume-up": (0x0C, 0xE9, 0.05),
    "volume-down": (0x0C, 0xEA, 0.05),
    "mute": (0x0C, 0xE2, 0.05),
    "siri": (0x0C, 0xCF, 1.0),
    "keyboard-toggle": (0x0C, 0x01AE, 0.05),
}

DOM_CODE_TO_HID = {
    "KeyA": 4, "KeyB": 5, "KeyC": 6, "KeyD": 7, "KeyE": 8, "KeyF": 9,
    "KeyG": 10, "KeyH": 11, "KeyI": 12, "KeyJ": 13, "KeyK": 14, "KeyL": 15,
    "KeyM": 16, "KeyN": 17, "KeyO": 18, "KeyP": 19, "KeyQ": 20, "KeyR": 21,
    "KeyS": 22, "KeyT": 23, "KeyU": 24, "KeyV": 25, "KeyW": 26, "KeyX": 27,
    "KeyY": 28, "KeyZ": 29,
    "Digit1": 30, "Digit2": 31, "Digit3": 32, "Digit4": 33, "Digit5": 34,
    "Digit6": 35, "Digit7": 36, "Digit8": 37, "Digit9": 38, "Digit0": 39,
    "Enter": 40, "NumpadEnter": 40, "Escape": 41, "Backspace": 42, "Tab": 43, "Space": 44,
    "Minus": 45, "Equal": 46, "BracketLeft": 47, "BracketRight": 48,
    "Backslash": 49, "Semicolon": 51, "Quote": 52, "Backquote": 53,
    "Comma": 54, "Period": 55, "Slash": 56, "CapsLock": 57,
    "ArrowRight": 79, "ArrowLeft": 80, "ArrowDown": 81, "ArrowUp": 82,
    "Delete": 76, "Home": 74, "End": 77, "PageUp": 75, "PageDown": 78,
    "Numpad0": 98, "Numpad1": 89, "Numpad2": 90, "Numpad3": 91, "Numpad4": 92,
    "Numpad5": 93, "Numpad6": 94, "Numpad7": 95, "Numpad8": 96, "Numpad9": 97,
    "NumpadDecimal": 99, "NumpadAdd": 87, "NumpadSubtract": 86, "NumpadMultiply": 85,
    "NumpadDivide": 84,
}

STOCK_BUNDLES = {
    "com.apple.Preferences",
    "com.apple.camera",
    "com.apple.AppStore",
    "com.apple.mobilenotes",
    "com.apple.calculator",
    "com.apple.mobilecal",
    "com.apple.mobiletimer",
    "com.apple.DocumentsApp",
    "com.apple.MobileSMS",
    "com.apple.mail",
    "com.apple.Maps",
    "com.apple.mobilesafari",
    "com.apple.mobileslideshow",
    "com.apple.Music",
    "com.apple.weather",
    "com.apple.reminders",
    "com.apple.Health",
    "com.apple.Wallet",
    "com.apple.facetime",
    "com.apple.MobileAddressBook",
    "com.apple.podcasts",
    "com.apple.iBooks",
    "com.apple.news",
    "com.apple.tv",
    "com.apple.Home",
    "com.apple.stocks",
    "com.apple.shortcuts",
    "com.apple.freeform",
    "com.apple.Journal",
    "com.apple.VoiceMemos",
    "com.apple.compass",
    "com.apple.measure",
    "com.apple.findmy",
    "com.apple.Fitness",
    "com.apple.Translate",
    "com.apple.Bridge",
}

def _icon_cache_dir() -> pathlib.Path:
    if sys.platform == "win32":
        appdata = os.environ.get("LOCALAPPDATA") or str(pathlib.Path.home() / "AppData" / "Local")
        return pathlib.Path(appdata) / "Meridian" / "cache" / "icons"
    return pathlib.Path.home() / ".cache" / "meridian" / "icons"


ICON_CACHE_DIR = _icon_cache_dir()
APPS_LOCK = threading.Lock()
APPS_STATE: dict[str, Any] = {"list": None}
ICON_MEM: dict[str, bytes] = {}

RSD_CACHE: dict[str, Any] = {"rsd": None, "udid": None}
ACTION_CTX: dict[str, Any] = {
    "indigo": None,
    "hid": None,
    "send_lock": None,
    "kbd_lock": None,
    "kbd_id": None,
    "held_keys": set(),
    "held_mods": 0,
    "kb_attempt": 0,
    "want_kb": False,
    "recycle": False,
}
WDA_QUEUE: asyncio.Queue = asyncio.Queue()
RS: dict[str, Any] = {"loop": None, "queue": None}


def norm(v: float) -> int:
    return max(0, min(65535, round(v * 65535)))


async def _rsd_for_device(udid: Optional[str] = None):
    if RSD_CACHE["rsd"] is not None:
        return RSD_CACHE["rsd"]
    rsds = await get_tunneld_devices(("127.0.0.1", 49151))
    if not rsds:
        raise RuntimeError("tunneld has no devices connected")
    if udid:
        rsd = next((r for r in rsds if getattr(r, "udid", None) == udid), None)
    else:
        rsd = rsds[0]
    if rsd is None:
        raise RuntimeError(f"tunneld cannot find device {udid}")
    RSD_CACHE["rsd"] = rsd
    RSD_CACHE["udid"] = getattr(rsd, "udid", None)
    return rsd


async def _fetch_apps_async():
    rsd = await _rsd_for_device(RSD_CACHE["udid"])
    async with AppServiceService(rsd) as svc:
        raw = await svc.list_apps(
            include_app_clips=False,
            include_removable_apps=True,
            include_hidden_apps=False,
            include_internal_apps=False,
            include_default_apps=True,
        )
    out = []
    seen = set()
    n_user = 0
    for a in raw:
        bid = a.get("bundleIdentifier") or a.get("CFBundleIdentifier")
        if not bid or bid in seen:
            continue
        bpath = str(a.get("applicationBundlePath") or a.get("path") or "")
        plow = bpath.lower()
        kind = str(a.get("applicationType") or "").lower()
        is_user = (
            kind == "user"
            or bool(a.get("isRemovable") or a.get("removable"))
            or ("/containers/bundle/application/" in plow and ".staged" not in plow)
        )
        if not is_user and bid not in STOCK_BUNDLES:
            continue
        name = str(a.get("displayName") or a.get("name") or "").strip()
        if not name:
            continue
        exe = bpath.split(".app/")[-1].strip("/") if ".app/" in bpath else ""
        out.append({"bundleId": bid, "name": name, "exe": exe, "user": is_user})
        seen.add(bid)
        if is_user:
            n_user += 1

    out.sort(key=lambda e: (0 if e.pop("user") else 1, e["name"].lower()))
    log.info("app list loaded: %d apps (%d user, %d stock)", len(out), n_user, len(out) - n_user)
    return out


def _token_name(tok: Any) -> str:
    for k in ("name", "executablePath", "executable"):
        if hasattr(tok, k):
            v = getattr(tok, k)
            if v:
                return str(v).rsplit("/", 1)[-1]
    return ""


async def _fetch_running_async():
    rsd = await _rsd_for_device(RSD_CACHE["udid"])
    async with AppServiceService(rsd) as svc:
        procs = await svc.list_processes()

    with APPS_LOCK:
        apps = APPS_STATE["list"] or []
    exe_to_bid = {a["exe"].lower(): a["bundleId"] for a in apps if a.get("exe")}
    bid_exact = {a["bundleId"] for a in apps}

    running = []
    seen = set()
    for p in procs:
        pid = getattr(p, "pid", None)
        if pid is None or pid in seen:
            continue
        seen.add(pid)
        name = _token_name(p)
        matched_bid = None
        if name in bid_exact:
            matched_bid = name
        elif name.lower() in exe_to_bid:
            matched_bid = exe_to_bid[name.lower()]

        if matched_bid:
            running.append({"bundleId": matched_bid, "pid": pid})

    return running


async def _launch_app_async(bid: str):
    rsd = await _rsd_for_device(RSD_CACHE["udid"])
    async with AppServiceService(rsd) as svc:
        return await svc.launch_application(bid)


async def _kill_pid_async(pid: int):
    rsd = await _rsd_for_device(RSD_CACHE["udid"])
    async with AppServiceService(rsd) as svc:
        return await svc.kill_process(pid)


async def _fetch_icon_cached(bid: str) -> bytes:
    if bid in ICON_MEM:
        return ICON_MEM[bid]
    ICON_CACHE_DIR.mkdir(parents=True, exist_ok=True)
    fpath = ICON_CACHE_DIR / f"{bid}.png"
    if fpath.exists() and fpath.stat().st_size > 500:
        raw = fpath.read_bytes()
        ICON_MEM[bid] = raw
        return raw

    rsd = await _rsd_for_device(RSD_CACHE["udid"])
    async with IconService(rsd) as svc:
        icon = await svc.fetch_icon(bid)
        data = getattr(icon, "png_data", None) or getattr(icon, "data", None)
        if not data:
            raise RuntimeError(f"no icon returned for {bid}")
        raw = bytes(data)
        fpath.write_bytes(raw)
        ICON_MEM[bid] = raw
        return raw


def run_on_loop(coro):
    loop = RS["loop"]
    if loop is None:
        raise RuntimeError("event loop not ready")
    fut = asyncio.run_coroutine_threadsafe(coro, loop)
    return fut.result(timeout=15)


# ---- WebSocket Framing ------------------------------------------------------

class SyncWS:
    def __init__(self, sock: socket.socket):
        self.sock = sock
        self.closed = False

    def close(self):
        if not self.closed:
            self.closed = True
            try:
                self.sock.close()
            except Exception:
                pass

    def send_text(self, s: str):
        self._frame(0x1, s.encode("utf-8"))

    def _frame(self, opcode: int, payload: bytes):
        if self.closed:
            return
        b0 = 0x80 | (opcode & 0x0F)
        ln = len(payload)
        if ln < 126:
            hdr = bytes([b0, ln])
        elif ln < 65536:
            hdr = bytes([b0, 126]) + ln.to_bytes(2, "big")
        else:
            hdr = bytes([b0, 127]) + ln.to_bytes(8, "big")
        try:
            self.sock.sendall(hdr + payload)
        except Exception:
            self.close()


def read_ws_frames(sock, ws, on_message):
    buf = b""
    try:
        while not ws.closed:
            chunk = sock.recv(4096)
            if not chunk:
                break
            buf += chunk
            while len(buf) >= 2:
                b0, b1 = buf[0], buf[1]
                op = b0 & 0x0F
                masked = bool(b1 & 0x80)
                ln = b1 & 0x7F
                off = 2
                if ln == 126:
                    if len(buf) < 4:
                        break
                    ln = int.from_bytes(buf[off : off + 2], "big")
                    off += 2
                elif ln == 127:
                    if len(buf) < 10:
                        break
                    ln = int.from_bytes(buf[off : off + 8], "big")
                    off += 8
                mk = b""
                if masked:
                    if len(buf) - off < 4:
                        break
                    mk = buf[off : off + 4]
                    off += 4
                if len(buf) - off < ln:
                    break
                payload = buf[off : off + ln]
                if masked and mk:
                    payload = bytes(p ^ mk[i % 4] for i, p in enumerate(payload))
                buf = buf[off + ln :]
                if op == 0x8:
                    ws.close()
                    return
                if op == 0x9:
                    ws._frame(0xA, payload)
                    continue
                on_message(op, payload)
    except OSError:
        pass
    finally:
        ws.close()


# ---- HTTP + WS Request Handler ---------------------------------------------

class BridgeHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _json(self, code: int, obj: Any):
        body = json.dumps(obj).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "*")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self):
        self.send_response(200)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "*")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_HEAD(self):
        self.do_GET()

    def do_GET(self):
        path = self.path.split("?")[0]
        if path == "/apps.json":
            with APPS_LOCK:
                cached = APPS_STATE["list"]
            if cached is None:
                try:
                    cached = run_on_loop(_fetch_apps_async())
                    with APPS_LOCK:
                        APPS_STATE["list"] = cached
                except Exception as e:
                    self._json(502, {"error": f"fetch failed: {e}", "apps": []})
                    return
            self._json(200, {"apps": cached})
            return

        if path == "/apps/running.json":
            try:
                running = run_on_loop(_fetch_running_async())
                self._json(200, {"running": running})
            except Exception as e:
                self._json(502, {"error": str(e), "running": []})
            return

        if path.startswith("/icon/"):
            bid = urllib.parse.unquote(path[len("/icon/") :].removesuffix(".png"))
            try:
                raw = run_on_loop(_fetch_icon_cached(bid))
                self.send_response(200)
                self.send_header("Content-Type", "image/png")
                self.send_header("Access-Control-Allow-Origin", "*")
                self.send_header("Cache-Control", "public, max-age=604800")
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)
            except Exception as e:
                self._json(404, {"error": f"no icon for {bid}: {e}"})
            return

        if path == "/ws":
            key = self.headers.get("Sec-WebSocket-Key", "")
            accept = base64.b64encode(hashlib.sha1((key + WS_GUID).encode()).digest()).decode()
            self.send_response(101)
            self.send_header("Upgrade", "websocket")
            self.send_header("Connection", "Upgrade")
            self.send_header("Sec-WebSocket-Accept", accept)
            self.end_headers()

            ws = SyncWS(self.connection)

            def on_msg(op: int, payload: bytes):
                if op != 0x1:
                    return
                try:
                    job = json.loads(payload.decode("utf-8"))
                except Exception:
                    return
                loop, q = RS["loop"], RS["queue"]
                if loop and q is not None:
                    loop.call_soon_threadsafe(q.put_nowait, job)

            reader = threading.Thread(target=read_ws_frames, args=(self.connection, ws, on_msg), daemon=True)
            reader.start()

            while not ws.closed:
                time.sleep(0.05)
            self.close_connection = True
            return

        self._json(404, {"error": "not found"})

    def do_POST(self):
        path = self.path.split("?")[0]
        if path.startswith("/app/launch/"):
            bid = urllib.parse.unquote(path[len("/app/launch/") :])
            try:
                run_on_loop(_launch_app_async(bid))
                self._json(200, {"ok": True})
            except Exception as e:
                self._json(502, {"error": f"launch failed: {e}"})
            return

        if path.startswith("/app/kill/"):
            try:
                pid = int(urllib.parse.unquote(path.rsplit("/", 1)[1]))
                run_on_loop(_kill_pid_async(pid))
                self._json(200, {"ok": True})
            except Exception as e:
                self._json(502, {"error": f"kill failed: {e}"})
            return

        self._json(404, {"error": "not found"})

    def log_message(self, *args):
        pass


_cached_wda_sid: Optional[str] = None
_wda_lock = threading.Lock()


def _get_wda_session_id() -> Optional[str]:
    global _cached_wda_sid
    if _cached_wda_sid:
        return _cached_wda_sid
    try:
        s_req = urllib.request.Request(
            "http://127.0.0.1:8100/session",
            data=b'{"capabilities":{}}',
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(s_req, timeout=1.5) as r:
            data = json.loads(r.read() or b"{}")
            _cached_wda_sid = data.get("value", {}).get("sessionId") or data.get("sessionId")
            return _cached_wda_sid
    except Exception:
        return None


def _send_wda_text_sync(text: str):
    global _cached_wda_sid
    with _wda_lock:
        for _ in range(2):
            sid = _get_wda_session_id()
            if not sid:
                return
            try:
                k_req = urllib.request.Request(
                    f"http://127.0.0.1:8100/session/{sid}/wda/keys",
                    data=json.dumps({"value": [text]}).encode("utf-8"),
                    headers={"Content-Type": "application/json"},
                )
                with urllib.request.urlopen(k_req, timeout=1.5) as resp:
                    if resp.status == 200:
                        return
            except Exception:
                _cached_wda_sid = None


def _is_keyboard_displayed() -> Optional[bool]:
    """Check if the on-screen software keyboard is currently visible on the device."""
    sid = _get_wda_session_id()
    if not sid:
        return None
    try:
        req = urllib.request.Request(
            f"http://127.0.0.1:8100/session/{sid}/element",
            data=b'{"using":"class name","value":"XCUIElementTypeKeyboard"}',
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(req, timeout=1.0) as r:
            data = json.loads(r.read())
            eid = data.get("value", {}).get("ELEMENT")
            if not eid:
                return False
        req2 = urllib.request.Request(f"http://127.0.0.1:8100/session/{sid}/element/{eid}/displayed")
        with urllib.request.urlopen(req2, timeout=1.0) as r2:
            return bool(json.loads(r2.read()).get("value"))
    except Exception:
        return None


async def _set_soft_keyboard_visibility(target_visible: bool):
    """Ensure in-device software keyboard matches target_visible (True=visible, False=hidden)."""
    loop = asyncio.get_running_loop()
    displayed = await loop.run_in_executor(None, _is_keyboard_displayed)
    ibtn = ACTION_CTX.get("indigo")
    if ibtn and displayed is not None:
        if displayed != target_visible:
            log.info("soft keyboard displayed=%s, target=%s -> toggling via 0x0C, 0x01AE", displayed, target_visible)
            await ibtn.send_button(0x0C, 0x01AE, HID_BUTTON_STATE_DOWN)
            await asyncio.sleep(0.04)
            await ibtn.send_button(0x0C, 0x01AE, HID_BUTTON_STATE_UP)


# ---- 60Hz CoreDevice HID Touch Digitizer Stream ------------------------------

async def gesture_worker(queue: asyncio.Queue, udid: Optional[str] = None):
    ACTION_CTX["worker_task"] = asyncio.current_task()
    while True:
        try:
            rsd = await _rsd_for_device(udid)
            indigo = IndigoHIDService(rsd)
            async with indigo as ibtn:
                log.info("CoreDevice button channel armed")
                ACTION_CTX["indigo"] = ibtn
                try:
                    async with touch_session(rsd) as hid:
                        log.info("60Hz native capacitive touchscreen digitizer active")
                        ACTION_CTX["hid"] = hid
                        # Mount virtual hardware keyboard ONCE per session to suppress on-screen keyboard
                        try:
                            kid = await hid.create_keyboard_service()
                            ACTION_CTX["kbd_id"] = kid
                            log.info("✓ Hardware virtual keyboard mounted permanently (id=%s)", kid)
                        except Exception as e:
                            log.warning("could not mount virtual keyboard, falling back to 512: %s", e)
                            ACTION_CTX["kbd_id"] = 512
                        await consume_and_stream(queue, hid)
                finally:
                    ACTION_CTX["indigo"] = None
                    ACTION_CTX["hid"] = None
                    ACTION_CTX["kbd_id"] = None
                    ACTION_CTX["held_keys"].clear()
                    ACTION_CTX["held_mods"] = 0
        except asyncio.CancelledError:
            log.info("gesture worker reset requested — re-arming...")
            await asyncio.sleep(0.5)
            continue
        except Exception as e:
            RSD_CACHE["rsd"] = None
            log.warning("HID gesture worker dropped (%s) — reconnecting in 1s...", e)
            await asyncio.sleep(1.0)


def reset_gesture_worker():
    """Cancel the current gesture worker loop so it immediately reconnects to newly attached RSD."""
    RSD_CACHE["rsd"] = None
    loop = RS.get("loop")
    task = ACTION_CTX.get("worker_task")
    if loop and task and not task.done():
        loop.call_soon_threadsafe(task.cancel)


async def consume_and_stream(queue: asyncio.Queue, hid: Any):
    send_lock = ACTION_CTX["send_lock"]
    state = {"down": False, "x": norm(0.5), "y": norm(0.5)}
    lnx = None
    lny = None
    snx = 0.0
    sny = 0.0
    last_move_wall = 0.0
    settling = False
    settled_done = False

    async def send_contact(x: int, y: int):
        send_lock = ACTION_CTX.get("send_lock")
        if not send_lock:
            return
        try:
            async with asyncio.timeout(0.08):
                async with send_lock:
                    await hid.send_touchscreen(TOUCHSCREEN_STATE_CONTACT, x, y)
        except Exception as e:
            log.debug("send_contact error: %s", e)

    async def send_release(x: int, y: int):
        send_lock = ACTION_CTX.get("send_lock")
        if not send_lock:
            return
        try:
            async with asyncio.timeout(0.08):
                async with send_lock:
                    await hid.send_touchscreen(TOUCHSCREEN_STATE_RELEASE, x, y)
        except Exception as e:
            log.debug("send_release error: %s", e)

    async def perform_action(name: str):
        entry = NAMED_BUTTONS.get(name)
        if entry is None:
            return
        indigo = ACTION_CTX.get("indigo")
        if indigo is None:
            return
        usage_page, usage_code, hold = entry
        try:
            async with asyncio.timeout(1.0):
                await indigo.send_button(usage_page, usage_code, HID_BUTTON_STATE_DOWN)
                await asyncio.sleep(hold)
                await indigo.send_button(usage_page, usage_code, HID_BUTTON_STATE_UP)
        except Exception as e:
            log.warning("perform_action %s error: %s", name, e)

    async def consumer():
        nonlocal lnx, lny, snx, sny, last_move_wall, settling, settled_done
        while True:
            try:
                job = await queue.get()
                kind = job.get("kind")
                if kind == "down":
                    fx = float(job.get("fx", 0.5))
                    fy = float(job.get("fy", 0.5))
                    state["x"] = norm(fx)
                    state["y"] = norm(fy)
                    state["down"] = True
                    log.debug("📍 Tap received at (fx=%.4f, fy=%.4f) -> raw (%d, %d)", fx, fy, state["x"], state["y"])
                    lnx = lny = None
                    snx = sny = 0.0
                    settling = False
                    settled_done = False
                    last_move_wall = time.monotonic()
                    await send_contact(state["x"], state["y"])
                elif kind == "move":
                    state["x"] = norm(job["fx"])
                    state["y"] = norm(job["fy"])
                    nx = norm(job["fx"])
                    ny = norm(job["fy"])
                    if lnx is not None:
                        ddx, ddy = nx - lnx, ny - lny
                        if abs(ddx) + abs(ddy) > 25:
                            snx, sny = ddx, ddy
                    lnx, lny = nx, ny
                    settling = False
                    last_move_wall = time.monotonic()
                elif kind == "release":
                    state["down"] = False
                    state["x"] = norm(job["fx"])
                    state["y"] = norm(job["fy"])
                    await send_release(state["x"], state["y"])
                elif kind == "action":
                    await perform_action(str(job.get("name") or ""))
                elif kind in ("key_down", "down_key", "key", "hid"):
                    code = str(job.get("code") or "")
                    key_str = str(job.get("key") or "")
                    shift = bool(job.get("shift", False))
                    ctrl = bool(job.get("ctrl", False))
                    alt = bool(job.get("alt", False))
                    meta = bool(job.get("meta", False))

                    kid = ACTION_CTX.get("kbd_id") or 512
                    usage = DOM_CODE_TO_HID.get(code)
                    if usage is None and key_str and key_str in ASCII_TO_HID:
                        usage, need_shift = ASCII_TO_HID[key_str]
                        if need_shift:
                            shift = True

                    is_special_unicode = bool(job.get("isSpecial")) or (
                        len(key_str) == 1 and (ord(key_str) > 127 or key_str in "¿¡€£¥§")
                    )

                    hid = ACTION_CTX.get("hid")
                    if hid and usage is not None and not is_special_unicode:
                        # Instant zero-latency hardware rollover press via native 39-byte bitmap
                        ACTION_CTX["held_keys"].add(usage)
                        active_usages = set(ACTION_CTX["held_keys"])
                        if shift:
                            active_usages.add(KEY_LEFT_SHIFT)
                        if ctrl:
                            active_usages.add(KEY_LEFT_CTRL)
                        if alt:
                            active_usages.add(KEY_LEFT_ALT)
                        if meta:
                            active_usages.add(KEY_LEFT_GUI)

                        send_lock = ACTION_CTX.get("send_lock")
                        if send_lock:
                            try:
                                async with asyncio.timeout(0.08):
                                    async with send_lock:
                                        await hid.send_keyboard(kid, active_usages)
                            except Exception as e:
                                log.debug("send_keyboard down error: %s", e)
                        else:
                            await hid.send_keyboard(kid, active_usages)
                    elif key_str:
                        text_to_send = key_str
                        if key_str == "Backspace":
                            text_to_send = "\b"
                        elif key_str == "Enter":
                            text_to_send = "\n"
                        elif key_str == "Tab":
                            text_to_send = "\t"
                        loop = RS.get("loop")
                        if loop:
                            # Asynchronous non-blocking dispatch so consumer queue never freezes
                            asyncio.create_task(loop.run_in_executor(None, _send_wda_text_sync, text_to_send))

                elif kind in ("key_up", "up_key"):
                    code = str(job.get("code") or "")
                    key_str = str(job.get("key") or "")
                    kid = ACTION_CTX.get("kbd_id") or 512
                    usage = DOM_CODE_TO_HID.get(code)
                    if usage is None and key_str and key_str in ASCII_TO_HID:
                        usage, _ = ASCII_TO_HID[key_str]

                    hid = ACTION_CTX.get("hid")
                    if hid and usage is not None:
                        ACTION_CTX["held_keys"].discard(usage)
                        active_usages = set(ACTION_CTX["held_keys"])
                        if job.get("shift"):
                            active_usages.add(KEY_LEFT_SHIFT)
                        if job.get("ctrl"):
                            active_usages.add(KEY_LEFT_CTRL)
                        if job.get("alt"):
                            active_usages.add(KEY_LEFT_ALT)
                        if job.get("meta"):
                            active_usages.add(KEY_LEFT_GUI)

                        send_lock = ACTION_CTX.get("send_lock")
                        if send_lock:
                            try:
                                async with asyncio.timeout(0.08):
                                    async with send_lock:
                                        await hid.send_keyboard(kid, active_usages)
                            except Exception as e:
                                log.debug("send_keyboard up error: %s", e)
                        else:
                            await hid.send_keyboard(kid, active_usages)

                elif kind == "paste":
                    text = str(job.get("text") or "")
                    if text:
                        loop = RS.get("loop")
                        if loop:
                            asyncio.create_task(loop.run_in_executor(None, _send_wda_text_sync, text))

                elif kind == "keyboard":
                    on = bool(job.get("on", False))
                    log.info("typing mode toggled: on=%s", on)
                    kid = ACTION_CTX.get("kbd_id") or 512
                    hid = ACTION_CTX.get("hid")
                    ibtn = ACTION_CTX.get("indigo")

                    # Release all held keys when turning off
                    if not on and hid:
                        send_lock = ACTION_CTX.get("send_lock")
                        try:
                            if send_lock:
                                async with asyncio.timeout(0.08):
                                    async with send_lock:
                                        await hid.send_keyboard(kid, [])
                            else:
                                await hid.send_keyboard(kid, [])
                        except Exception:
                            pass
                    ACTION_CTX["held_keys"].clear()
                    ACTION_CTX["held_mods"] = 0

                    # When turning OFF typing mode, restore the in-device keyboard for touch input
                    if not on and ibtn:
                        try:
                            await ibtn.send_button(0x0C, 0x01AE, HID_BUTTON_STATE_DOWN)
                            await asyncio.sleep(0.04)
                            await ibtn.send_button(0x0C, 0x01AE, HID_BUTTON_STATE_UP)
                            log.info("✓ In-device keyboard restored via 0x0C, 0x01AE")
                        except Exception as e:
                            log.debug("could not restore in-device keyboard: %s", e)
            except asyncio.CancelledError:
                break
            except Exception as e:
                log.warning("error in consumer job: %s", e)

    task = asyncio.create_task(consumer())
    try:
        period = 1.0 / STREAM_HZ
        while True:
            await asyncio.sleep(period)
            if not state["down"]:
                continue
            now = time.monotonic()
            active = (now - last_move_wall) < 0.12

            if active:
                settling = False
                jx = max(0, min(65535, state["x"] + random.randint(-2, 2)))
                jy = max(0, min(65535, state["y"] + random.randint(-2, 2)))
                await send_contact(jx, jy)
            elif not settled_done and (abs(snx) + abs(sny)) > 60:
                # cursor froze mid-gesture: ease-out along last direction for fluid inertia
                settling = True
                gx = float(state["x"])
                gy = float(state["y"])
                sxg = snx * 0.30
                syg = sny * 0.30
                for _i in range(14):
                    await asyncio.sleep(0.016)
                    gx += sxg
                    gy += syg
                    sxg *= 0.70
                    syg *= 0.70
                    gx = min(65535, max(0, gx))
                    gy = min(65535, max(0, gy))
                    await send_contact(round(gx), round(gy))
                state["x"] = int(min(65535, max(0, gx)))
                state["y"] = int(min(65535, max(0, gy)))
                settled_done = True
                settling = False
            else:
                # stationary hold: gentle micro-jitter keeps contact active
                jx = max(0, min(65535, state["x"] + random.randint(-1, 1)))
                jy = max(0, min(65535, state["y"] + random.randint(-1, 1)))
                await send_contact(jx, jy)
    finally:
        task.cancel()


class BridgeServer:
    """Standalone HTTP + WebSocket bridge server for a single device."""

    def __init__(self, host: str = "127.0.0.1", port: int = 9001, udid: Optional[str] = None):
        self.host = host
        self.port = port
        self.udid = udid
        self._srv: Optional[http.server.ThreadingHTTPServer] = None
        self._thread: Optional[threading.Thread] = None
        self._loop: Optional[asyncio.AbstractEventLoop] = None
        self._task: Optional[asyncio.Task] = None

    def start(self):
        self._thread = threading.Thread(target=self._run, daemon=True, name=f"bridge-{self.port}")
        self._thread.start()

    def stop(self):
        if self._srv:
            try:
                self._srv.shutdown()
                self._srv.server_close()
            except Exception:
                pass
        if self._loop and self._task and not self._task.done():
            self._loop.call_soon_threadsafe(self._task.cancel)

    def _run(self):
        async def _main():
            loop = asyncio.get_running_loop()
            self._loop = loop
            RS["loop"] = loop
            RS["queue"] = asyncio.Queue()
            ACTION_CTX["send_lock"] = asyncio.Lock()
            ACTION_CTX["kbd_lock"] = asyncio.Lock()

            self._task = asyncio.create_task(gesture_worker(RS["queue"], udid=self.udid))

            async def _warm():
                await asyncio.sleep(1.0)
                try:
                    lst = await _fetch_apps_async()
                    with APPS_LOCK:
                        APPS_STATE["list"] = lst
                except Exception as e:
                    log.debug("app list warmup: %s", e)

            asyncio.create_task(_warm())

            self._srv = http.server.ThreadingHTTPServer((self.host, self.port), BridgeHandler)
            log.info("✓ Meridian bridge ready: http://%s:%d/ (udid: %s)", self.host, self.port, self.udid or "auto")
            await loop.run_in_executor(None, self._srv.serve_forever)

        try:
            asyncio.run(_main())
        except (KeyboardInterrupt, SystemExit, asyncio.CancelledError):
            pass
        except Exception as e:
            log.error("bridge server on %s:%d failed: %s", self.host, self.port, e)


def run_remote_bridge(host: str = "127.0.0.1", port: int = 9001, udid: Optional[str] = None, api_url: str = ""):
    """Start the CoreDevice remote bridge and HTTP/WS server."""
    from .mesh import start_mesh, stop_mesh
    from .heartbeat import start_heartbeat_worker, stop_heartbeat_worker
    from .lifecycle import DeviceLifecycleManager
    log.info("starting Meridian remote control bridge on %s:%d...", host, port)

    # Start userspace Tailscale mesh sidecar (zero-sudo, ephemeral)
    start_mesh()

    # Start automated device lifecycle manager (hotplug, runner launch, auto-tap, tunnels)
    lifecycle = DeviceLifecycleManager(target_udid=udid, api_url=api_url)
    lifecycle.start()

    # Start device heartbeat reporter if UDID is known
    target_udid = udid or RSD_CACHE.get("udid")
    if target_udid:
        start_heartbeat_worker(target_udid, api_url=api_url)

    server = BridgeServer(host=host, port=port, udid=udid)
    server.start()

    try:
        while True:
            time.sleep(1.0)
    except (KeyboardInterrupt, SystemExit):
        log.info("stopping bridge")
    finally:
        server.stop()
        lifecycle.stop()
        stop_heartbeat_worker()
        stop_mesh()
