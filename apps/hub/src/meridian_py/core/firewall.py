"""Automatic Windows Firewall setup for Meridian device ports.

Opens the required port ranges on first launch so all connected iPhones
can be accessed remotely without manual firewall configuration.

On first launch, if not running as admin, prompts the user to elevate
via a Windows message box. Subsequent launches skip via a flag file.
"""
from __future__ import annotations

import ctypes
import logging
import os
import subprocess
import sys
import tempfile
from pathlib import Path

log = logging.getLogger(__name__)

# Port ranges for up to MAX_SLOTS devices (see core/slots.py)
MAX_SLOTS = 32
WDA_RANGE = (8100, 8100 + MAX_SLOTS - 1)       # 8100-8131
BRIDGE_RANGE = (9001, 9001 + MAX_SLOTS - 1)    # 9001-9032
STREAM_RANGE = (9200, 9200 + MAX_SLOTS - 1)    # 9200-9231
TUNNELD_PORT = 49151

_RULES = [
    ("Meridian-WDA", WDA_RANGE, "Meridian WDA (WebDeviceAgent) port range"),
    ("Meridian-Bridge", BRIDGE_RANGE, "Meridian HID Touch Bridge port range"),
    ("Meridian-Stream", STREAM_RANGE, "Meridian Screen Stream port range"),
    ("Meridian-Tunneld", (TUNNELD_PORT, TUNNELD_PORT), "Meridian pymobiledevice3 tunneld"),
]

_FLAG_FILE = Path(os.environ.get("LOCALAPPDATA", "")) / "Meridian" / "state" / ".firewall_done"

_MB_YESNO = 0x00000004
_MB_ICONWARNING = 0x00000030
_IDYES = 6


def is_admin() -> bool:
    """Check if the current process has administrator privileges."""
    if sys.platform != "win32":
        return True
    try:
        return bool(ctypes.windll.shell32.IsUserAnAdmin())
    except Exception:
        return False


def _already_configured() -> bool:
    if sys.platform != "win32":
        return True
    return _FLAG_FILE.exists()


def _mark_done() -> None:
    _FLAG_FILE.parent.mkdir(parents=True, exist_ok=True)
    _FLAG_FILE.write_text("ok")


def _build_netsh_commands() -> str:
    """Build netsh commands for all rules."""
    lines = []
    for name, (port_start, port_end), desc in _RULES:
        port_str = f"{port_start}-{port_end}" if port_start != port_end else str(port_start)
        lines.append(
            f'netsh advfirewall firewall add rule name="{name}" dir=in action=allow '
            f'protocol=TCP localport={port_str} description="{desc}" enable=yes'
        )
    return " & ".join(lines)


def _run_firewall_rules_direct() -> bool:
    """Add rules directly (when already running as admin)."""
    all_ok = True
    for name, (port_start, port_end), desc in _RULES:
        port_str = f"{port_start}-{port_end}" if port_start != port_end else str(port_start)
        cmd = [
            "netsh", "advfirewall", "firewall", "add", "rule",
            f"name={name}", "dir=in", "action=allow", "protocol=TCP",
            f"localport={port_str}", f"description={desc}", "enable=yes",
        ]
        try:
            result = subprocess.run(cmd, capture_output=True, text=True, timeout=10,
                                    creationflags=0x08000000)
            if result.returncode != 0:
                log.warning("firewall rule failed: %s — %s", name, result.stdout.strip())
                all_ok = False
            else:
                log.info("firewall rule added: %s", name)
        except Exception as e:
            log.warning("firewall rule error: %s — %s", name, e)
            all_ok = False

    if all_ok:
        _mark_done()
        log.info("Windows Firewall rules configured")
    else:
        log.warning("some firewall rules failed")
    return all_ok


def _elevate_and_add_rules() -> bool:
    """
    Write a temp .bat script and elevate it via ShellExecuteW("runas").
    The .bat approach avoids shell-escaping issues with -c arguments.
    """
    bat_content = _build_netsh_commands()
    bat_path = Path(tempfile.gettempdir()) / "meridian_firewall_setup.bat"
    bat_path.write_text(bat_content, encoding="utf-8")

    try:
        result = ctypes.windll.shell32.ShellExecuteW(
            None, "runas", str(bat_path), None, None, 0,  # SW_HIDE
        )
        if result > 32:
            _mark_done()
            log.info("firewall rules queued via elevated .bat (UAC accepted)")
            return True
        else:
            log.warning("UAC elevation denied (result=%d)", result)
            return False
    except Exception as e:
        log.warning("elevation error: %s", e)
        return False
    finally:
        # Don't delete immediately — the elevated process may still be reading it
        # Schedule deletion via a temp cleanup mechanism
        try:
            # Try to delete after a short delay
            import threading
            def _cleanup():
                import time
                time.sleep(3)
                try:
                    bat_path.unlink(missing_ok=True)
                except Exception:
                    pass
            threading.Thread(target=_cleanup, daemon=True).start()
        except Exception:
            pass


def ensure_firewall_rules() -> bool:
    """
    Ensure all Meridian firewall rules exist.

    First launch: shows a message box asking user to approve elevation.
    If approved, writes a temp .bat and elevates it via ShellExecuteW("runas").
    Subsequent launches skip entirely (flag file).
    On non-Windows, this is a no-op.
    """
    if sys.platform != "win32":
        return True

    if _already_configured():
        return True

    # If already admin, add rules directly
    if is_admin():
        return _run_firewall_rules_direct()

    # Not admin — ask user to elevate
    msg = (
        "Meridian needs to open firewall ports for iPhone remote access.\n\n"
        "This requires one-time administrator approval.\n"
        "Click Yes to continue, or No to skip (remote access won't work)."
    )
    answer = ctypes.windll.user32.MessageBoxW(None, msg, "Meridian — Firewall Setup", _MB_YESNO | _MB_ICONWARNING)

    if answer == _IDYES:
        return _elevate_and_add_rules()
    else:
        log.info("user declined firewall setup")
        _mark_done()
        return False


def remove_firewall_rules() -> None:
    """Remove all Meridian firewall rules (for clean uninstall)."""
    if sys.platform != "win32":
        return
    if not is_admin():
        log.warning("must be admin to remove firewall rules")
        return

    for name, _, _ in _RULES:
        try:
            subprocess.run(
                ["netsh", "advfirewall", "firewall", "delete", "rule", f"name={name}"],
                capture_output=True, timeout=5,
                creationflags=0x08000000,
            )
        except Exception:
            pass
    if _FLAG_FILE.exists():
        _FLAG_FILE.unlink()
    log.info("Meridian firewall rules removed")
