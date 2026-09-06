#!/usr/bin/env python3
"""Entrypoint launcher for PyInstaller bundled Meridian binary."""
import os
import sys
from pathlib import Path


def _setup_windowed_mode():
    """In windowed (no-console) mode, redirect stdout/stderr so print() doesn't crash."""
    if sys.platform != "win32":
        return
    try:
        if sys.stdout and hasattr(sys.stdout, "fileno"):
            sys.stdout.fileno()
            return
    except (AttributeError, OSError):
        pass

    log_dir = Path(os.environ.get("LOCALAPPDATA", os.path.expanduser("~"))) / "Meridian" / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / "meridian.log"

    class _TeeWriter:
        def __init__(self, log_file, fallback):
            self._log = open(log_file, "a", encoding="utf-8", errors="replace")
            self._fallback = fallback
            self.encoding = "utf-8"

        def write(self, msg):
            if msg:
                try:
                    self._log.write(msg)
                    self._log.flush()
                except Exception:
                    pass
            try:
                self._fallback.write(msg)
            except Exception:
                pass

        def flush(self):
            try:
                self._log.flush()
            except Exception:
                pass

        def fileno(self):
            try:
                return self._fallback.fileno()
            except Exception:
                return -1

        def isatty(self):
            return False

    nul = open(os.devnull, "w", encoding="utf-8", errors="replace")
    tee = _TeeWriter(log_path, nul)
    sys.stdout = tee
    sys.stderr = tee


_setup_windowed_mode()

from meridian_py.cli import main

if __name__ == "__main__":
    sys.exit(main())
