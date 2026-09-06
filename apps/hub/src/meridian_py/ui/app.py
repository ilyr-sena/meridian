"""
Desktop GUI Application Launcher for Meridian.
Detects display environment and launches PySide6 desktop interface,
with seamless fallback to headless CLI daemon for server environments.
"""
from __future__ import annotations

import logging
import os
import signal
import sys
from typing import Optional

from ..remote.orchestrator import MultiDeviceOrchestrator

log = logging.getLogger(__name__)


def is_gui_available() -> bool:
    """Check if graphical display is available."""
    if sys.platform == "win32":
        return True
    return bool(os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY"))


def run_desktop_app(cli_mode: bool = False, auto_start: bool = True, api_url: str = "") -> int:
    """Launch the Meridian desktop application or fallback to CLI daemon."""
    orchestrator = MultiDeviceOrchestrator(api_url=api_url)

    # Fallback to headless CLI daemon if explicitly requested or no display server found
    if cli_mode or not is_gui_available():
        log.info("🖥 Running in Headless CLI Daemon mode")
        orchestrator.start(auto_start_ready=auto_start)

        def _sig_handler(sig, frame):
            log.info("signal received, stopping orchestrator...")
            orchestrator.stop()
            sys.exit(0)

        signal.signal(signal.SIGINT, _sig_handler)
        if hasattr(signal, "SIGTERM"):
            signal.signal(signal.SIGTERM, _sig_handler)

        log.info("✓ Meridian Daemon running. Press Ctrl+C to terminate.")
        while True:
            try:
                import time
                time.sleep(1.0)
            except (KeyboardInterrupt, SystemExit):
                orchestrator.stop()
                break
        return 0

    # Prefer XCB (X11/XWayland) on Linux for rock-solid zero-tear rendering
    if sys.platform.startswith("linux") and "QT_QPA_PLATFORM" not in os.environ:
        os.environ["QT_QPA_PLATFORM"] = "xcb"

    # Launch PySide6 Desktop GUI
    from PySide6.QtCore import QTimer
    from PySide6.QtWidgets import QApplication
    from .main_window import MainWindow

    app = QApplication.instance() or QApplication(sys.argv)
    app.setApplicationName("Meridian")
    app.setOrganizationName("IUS")

    # Allow Python signals (Ctrl+C / SIGINT) to interrupt Qt event loop
    sig_timer = QTimer()
    sig_timer.timeout.connect(lambda: None)
    sig_timer.start(250)

    # Start multi-device orchestrator in background
    orchestrator.start(auto_start_ready=auto_start)

    window = MainWindow(orchestrator)
    window.show()

    ret = app.exec()
    orchestrator.stop()
    os._exit(ret)
