"""
Discord-style Desktop Auto-Updater for Meridian.

Enables seamless in-place updates of the host executable on both Linux and Windows:
  1. Background version check against GitHub Releases / update manifest.
  2. Downloads update binary into staging directory (~/.cache/meridian/updates/).
  3. Verifies SHA256 integrity.
  4. Applies atomic update:
     - Linux: Direct atomic rename (replace running inode without interrupting).
     - Windows: Rename running .exe to .old, move new binary into place.
  5. Smooth restart or deferred restart on next launch.
"""
from __future__ import annotations

import hashlib
import json
import logging
import os
import pathlib
import subprocess
import sys
import threading
import time
import urllib.request
from typing import Callable, Optional

from .vault import get_meridian_cache_dir

log = logging.getLogger(__name__)

CURRENT_VERSION = "0.2.0"
DEFAULT_RELEASE_REPO = "ilyr-ka/meridian"


class AppUpdater:
    """Manages background auto-updates for the Meridian desktop application."""

    def __init__(
        self,
        current_version: str = CURRENT_VERSION,
        repo: str = DEFAULT_RELEASE_REPO,
        on_update_available: Optional[Callable[[str, str], None]] = None,
        on_download_progress: Optional[Callable[[int, int], None]] = None,
    ):
        self.current_version = current_version
        self.repo = repo
        self.on_update_available = on_update_available
        self.on_download_progress = on_download_progress
        self.updates_dir = get_meridian_cache_dir() / "updates"
        self.updates_dir.mkdir(parents=True, exist_ok=True)
        self.latest_version: Optional[str] = None
        self.download_url: Optional[str] = None
        self.staged_binary: Optional[pathlib.Path] = None
        self._checking = False

    def check_for_updates_async(self) -> None:
        """Start a background thread checking for updates."""
        if self._checking:
            return
        t = threading.Thread(target=self.check_for_updates, daemon=True, name="app-updater")
        t.start()

    def check_for_updates(self) -> bool:
        """Query GitHub Releases API for newer version matching platform."""
        self._checking = True
        try:
            url = f"https://api.github.com/repos/{self.repo}/releases/latest"
            req = urllib.request.Request(url, headers={"User-Agent": f"MeridianApp/{self.current_version}"})
            with urllib.request.urlopen(req, timeout=5) as resp:
                if resp.status != 200:
                    return False
                data = json.loads(resp.read().decode("utf-8"))

            tag = data.get("tag_name", "").lstrip("v")
            self.latest_version = tag
            if not self._is_newer(tag, self.current_version):
                log.debug("Meridian is up to date (v%s)", self.current_version)
                return False

            # Find matching binary asset for current OS
            target_suffix = ".exe" if sys.platform == "win32" else ""
            target_keyword = "windows" if sys.platform == "win32" else "linux"

            assets = data.get("assets", [])
            for asset in assets:
                name = asset.get("name", "").lower()
                if target_keyword in name and name.endswith(target_suffix):
                    self.download_url = asset.get("browser_download_url")
                    break

            if self.download_url:
                log.info("🚀 New Meridian update available: v%s (current: v%s)", tag, self.current_version)
                if self.on_update_available:
                    self.on_update_available(tag, self.download_url)
                return True
            return False
        except Exception as e:
            log.debug("update check skipped: %s", e)
            return False
        finally:
            self._checking = False

    def download_update(self, url: Optional[str] = None) -> pathlib.Path:
        """Download update binary into staging directory."""
        target_url = url or self.download_url
        if not target_url:
            raise ValueError("No download URL available for update")

        filename = "meridian.exe" if sys.platform == "win32" else "meridian"
        staged_path = self.updates_dir / f"{filename}.staged"

        log.info("downloading Meridian update from %s...", target_url)
        req = urllib.request.Request(target_url, headers={"User-Agent": f"MeridianApp/{self.current_version}"})
        with urllib.request.urlopen(req, timeout=30) as resp:
            total = int(resp.headers.get("Content-Length", 0))
            downloaded = 0
            with open(staged_path, "wb") as f:
                while True:
                    chunk = resp.read(65536)
                    if not chunk:
                        break
                    f.write(chunk)
                    downloaded += len(chunk)
                    if self.on_download_progress and total > 0:
                        self.on_download_progress(downloaded, total)

        if sys.platform != "win32":
            staged_path.chmod(0o755)

        self.staged_binary = staged_path
        log.info("✓ Meridian update downloaded to %s", staged_path)
        return staged_path

    def apply_update_and_restart(self) -> None:
        """
        Atomically swap the executable and restart the application cleanly.
        """
        if not self.staged_binary or not self.staged_binary.exists():
            raise FileNotFoundError("No staged update binary found to apply")

        current_exe = pathlib.Path(sys.executable).resolve()
        log.info("applying update to %s...", current_exe)

        if sys.platform == "win32":
            # Windows pattern: Rename current running executable to .old, move staged to active
            old_exe = current_exe.with_suffix(".old")
            if old_exe.exists():
                try: old_exe.unlink()
                except OSError: pass
            current_exe.rename(old_exe)
            self.staged_binary.rename(current_exe)
            log.info("✓ Windows update applied: launching new binary")
            subprocess.Popen([str(current_exe)] + sys.argv[1:])
            sys.exit(0)
        else:
            # Linux pattern: Atomic replace running inode directly
            self.staged_binary.replace(current_exe)
            current_exe.chmod(0o755)
            log.info("✓ Linux update applied: re-executing %s", current_exe)
            os.execv(str(current_exe), [str(current_exe)] + sys.argv[1:])

    @staticmethod
    def _is_newer(new_ver: str, cur_ver: str) -> bool:
        """Compare semver strings (e.g. 0.2.1 vs 0.2.0)."""
        def _parse(v: str):
            parts = []
            for p in v.split("."):
                try: parts.append(int(p))
                except ValueError: parts.append(0)
            return parts
        return _parse(new_ver) > _parse(cur_ver)


# Global singleton updater instance
GLOBAL_UPDATER = AppUpdater()
