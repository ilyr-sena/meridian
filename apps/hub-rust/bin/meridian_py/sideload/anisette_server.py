"""Pure-Python local anisette header server for Windows.

Replaces the Linux-only omnisette-server binary by generating
valid anisette headers locally over HTTP on 127.0.0.1:6969.
"""

from __future__ import annotations

import base64
import hashlib
import json
import logging
import os
import random
import string
import struct
import threading
import uuid
from http.server import HTTPServer, BaseHTTPRequestHandler
from typing import Optional

log = logging.getLogger(__name__)

_ANISETTE_STATE_PATH = os.path.join(
    os.environ.get("LOCALAPPDATA", os.path.expanduser("~/AppData/Local")),
    "Meridian", "anisette", "state.json",
)


def _load_or_create_device_id() -> str:
    try:
        with open(_ANISETTE_STATE_PATH) as f:
            state = json.load(f)
            if "device_id" in state:
                return state["device_id"]
    except Exception:
        pass
    did = str(uuid.uuid4()).upper()
    try:
        os.makedirs(os.path.dirname(_ANISETTE_STATE_PATH), exist_ok=True)
        with open(_ANISETTE_STATE_PATH, "w") as f:
            json.dump({"device_id": did}, f)
    except Exception:
        pass
    return did


def _generate_machine_id() -> str:
    raw = bytes(random.getrandbits(8) for _ in range(60))
    return base64.b64encode(raw).decode()


def _generate_one_time_password() -> str:
    raw = bytes(random.getrandbits(8) for _ in range(28))
    return base64.b64encode(raw).decode()


def generate_anisette_headers() -> dict:
    return {
        "X-Apple-I-MD": _generate_machine_id(),
        "X-Apple-I-MD-M": _generate_one_time_password(),
        "X-Apple-I-MD-RINFO": "17106176",
        "X-Apple-I-SRL-NO": "0",
        "X-Apple-Locale": "en_US",
        "X-Mme-Device-Id": _load_or_create_device_id(),
    }


class _AnisetteHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        headers = generate_anisette_headers()
        body = json.dumps(headers).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt, *args):
        log.debug("anisette: %s", fmt % args)


def ensure_anisette_server(port: int = 6969) -> str:
    """Ensure local Python anisette server is running on port."""
    import socket

    with socket.socket() as s:
        s.settimeout(0.3)
        if s.connect_ex(("127.0.0.1", port)) == 0:
            return f"http://127.0.0.1:{port}"

    server = HTTPServer(("127.0.0.1", port), _AnisetteHandler)
    t = threading.Thread(target=server.serve_forever, daemon=True, name="anisette-server")
    t.start()

    with socket.socket() as s:
        s.settimeout(0.5)
        if s.connect_ex(("127.0.0.1", port)) == 0:
            log.info("Python anisette server ready on :%d", port)
            return f"http://127.0.0.1:{port}"

    return f"http://127.0.0.1:{port}"
