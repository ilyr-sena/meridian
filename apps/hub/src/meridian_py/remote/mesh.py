"""
Meridian Mesh: Zero-sudo userspace Tailscale sidecar (tsnet) integration.
Binds local phone ports (8100 WDA, 9001 bridge, 9200 probe) to a secure Tailnet.
"""
from __future__ import annotations

import json
import logging
import os
import shutil
import subprocess
import sys
import threading
from pathlib import Path
from typing import Optional

log = logging.getLogger(__name__)

_mesh_process: Optional[subprocess.Popen] = None
_current_mesh_ip: Optional[str] = None


def _auth_file_path() -> Path:
    if sys.platform == "win32":
        appdata = os.environ.get("LOCALAPPDATA") or str(Path.home() / "AppData" / "Local")
        return Path(appdata) / "Meridian" / "state" / "mesh_auth.json"
    return Path.home() / ".local" / "state" / "meridian" / "mesh_auth.json"


_auth_file = _auth_file_path()


def get_mesh_ip() -> Optional[str]:
    global _current_mesh_ip
    if _current_mesh_ip:
        return _current_mesh_ip
    # Auto-detect native Tailscale client IP if installed
    try:
        res = subprocess.run(["tailscale", "ip", "-4"], capture_output=True, text=True, timeout=1.0)
        if res.returncode == 0:
            ip = res.stdout.strip()
            if ip and ip.startswith("100."):
                return ip
    except Exception:
        pass
    return None


def get_stored_authkey() -> Optional[str]:
    env_key = os.environ.get("TS_AUTHKEY") or os.environ.get("MERIDIAN_MESH_KEY")
    if env_key:
        return env_key.strip()
    # Check vault for a stored mesh key
    try:
        from ..core.vault import GLOBAL_VAULT
        vault_key = GLOBAL_VAULT.get_mesh_key()
        if vault_key:
            return vault_key
    except Exception:
        pass
    if _auth_file.exists():
        try:
            data = json.loads(_auth_file.read_text())
            return data.get("authkey")
        except Exception:
            pass
    return None


def save_authkey(key: str) -> None:
    _auth_file.parent.mkdir(parents=True, exist_ok=True)
    _auth_file.write_text(json.dumps({"authkey": key.strip()}, indent=2))
    if sys.platform != "win32":
        _auth_file.chmod(0o600)


def find_mesh_binary() -> Optional[str]:
    exe_name = "meridian-mesh.exe" if sys.platform == "win32" else "meridian-mesh"

    # 0. PyInstaller bundled asset
    if hasattr(sys, "_MEIPASS"):
        bundled = Path(sys._MEIPASS) / exe_name
        if bundled.exists() and os.access(bundled, os.X_OK):
            return str(bundled)

    # 1. Look in PATH
    bin_path = shutil.which("meridian-mesh")
    if bin_path and os.access(bin_path, os.X_OK):
        return bin_path
    # 2. Look in package bin/ or ~/.local/bin
    local_bin = Path.home() / ".local" / "bin" / exe_name
    if local_bin.exists() and os.access(local_bin, os.X_OK):
        return str(local_bin)
    # 3. Look next to meridian-mesh source (sibling repo)
    source_bin = Path(__file__).resolve().parents[4] / "meridian-mesh" / exe_name
    if source_bin.exists() and os.access(source_bin, os.X_OK):
        return str(source_bin)
    # 4. Look in parent directory of meridian-py
    parent_bin = Path(__file__).resolve().parents[3] / "meridian-mesh" / exe_name
    if parent_bin.exists() and os.access(parent_bin, os.X_OK):
        return str(parent_bin)
    return None


def start_mesh(
    authkey: Optional[str] = None,
    hostname: Optional[str] = None,
    forwards: str = "8100:8100,9001:9001,9200:9200",
) -> Optional[subprocess.Popen]:
    global _mesh_process
    key = authkey or get_stored_authkey()
    if not key:
        log.info("no Tailscale auth key found; mesh sidecar will not start (use --mesh-key or TS_AUTHKEY)")
        return None

    binary = find_mesh_binary()
    if not binary:
        log.warning("meridian-mesh binary not found; mesh sidecar disabled")
        return None

    cmd = [binary, f"-authkey={key}", f"-forward={forwards}"]
    if hostname:
        cmd.append(f"-hostname={hostname}")

    try:
        _NO_WINDOW = 0x08000000 if sys.platform == "win32" else 0
        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            creationflags=_NO_WINDOW,
        )
        _mesh_process = proc

        def _reader():
            global _current_mesh_ip
            tail_ip = None
            tail_host = None
            for line in proc.stdout:
                line_str = line.strip()
                if "Tailscale IP:" in line_str:
                    tail_ip = line_str.split("Tailscale IP:")[-1].strip()
                    _current_mesh_ip = tail_ip
                elif "Tailscale Host:" in line_str:
                    tail_host = line_str.split("Tailscale Host:")[-1].strip()

                if tail_ip and tail_host:
                    log.info("🌐 Meridian Mesh Live: IP=%s | Host=%s", tail_ip, tail_host)
                    print(f"\n==================================================", flush=True)
                    print(f"🚀 Meridian Remote Mesh Active!", flush=True)
                    print(f"   Tailscale IP:   {tail_ip}", flush=True)
                    print(f"   Tailscale Host: {tail_host}", flush=True)
                    print(f"   Ports exposed:  8100 (WDA), 9001 (Bridge), 9200 (Stream)", flush=True)
                    print(f"==================================================\n", flush=True)
                    tail_ip = None
                    tail_host = None
                elif "error" in line_str.lower() or "failed" in line_str.lower():
                    log.warning("[mesh] %s", line_str)

        t = threading.Thread(target=_reader, daemon=True)
        t.start()
        return proc
    except Exception as e:
        log.error("failed to start meridian-mesh sidecar: %s", e)
        return None


def stop_mesh():
    global _mesh_process
    if _mesh_process and _mesh_process.poll() is None:
        try:
            _mesh_process.terminate()
            _mesh_process.wait(timeout=3)
        except Exception:
            _mesh_process.kill()
        _mesh_process = None
