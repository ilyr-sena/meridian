"""
Main Window for the Meridian Desktop Application.
Features:
  - Header with Tailscale Mesh IP, global stats, and Start All / Stop All controls.
  - Interactive auto-updater notification bar.
  - Responsive grid/card layout of connected devices.
  - Live activity log console.
"""
from __future__ import annotations

import logging
from typing import Dict, Optional

from PySide6.QtCore import Qt, QTimer, Signal
from PySide6.QtWidgets import (
    QFrame,
    QHBoxLayout,
    QLabel,
    QMainWindow,
    QPushButton,
    QScrollArea,
    QSplitter,
    QVBoxLayout,
    QWidget,
)

from ..core.inspector import DeviceReport, DeviceState
from ..core.updater import CURRENT_VERSION, GLOBAL_UPDATER
from ..remote.mesh import get_mesh_ip
from ..remote.orchestrator import MultiDeviceOrchestrator
from .device_card import DeviceCard
from .log_console import LogConsoleWidget
from .theme import DARK_STYLE

log = logging.getLogger(__name__)


class MainWindow(QMainWindow):
    """Primary Meridian Desktop Hub application window."""

    device_changed_signal = Signal(object)
    update_available_signal = Signal(str, str)

    def __init__(self, orchestrator: MultiDeviceOrchestrator):
        super().__init__()
        self.orchestrator = orchestrator
        self.setWindowTitle(f"Meridian Hub v{CURRENT_VERSION}")
        self.resize(860, 680)
        self.setStyleSheet(DARK_STYLE)

        self._cards: Dict[str, DeviceCard] = {}

        self._init_ui()
        self._setup_orchestrator()
        self._setup_updater()

    def _init_ui(self):
        central = QWidget()
        self.setCentralWidget(central)
        main_layout = QVBoxLayout(central)
        main_layout.setContentsMargins(20, 20, 20, 20)
        main_layout.setSpacing(14)

        # 1. Update Notification Banner (Hidden by default)
        self.update_banner = QFrame()
        self.update_banner.setStyleSheet("background-color: #2e1065; border: 1px solid #7c3aed; border-radius: 8px; padding: 6px 12px;")
        self.update_banner.setVisible(False)
        ub_layout = QHBoxLayout(self.update_banner)
        ub_layout.setContentsMargins(8, 4, 8, 4)

        self.update_lbl = QLabel("🚀 A new version of Meridian is available!")
        self.update_lbl.setStyleSheet("color: #ddd6fe; font-weight: 500;")
        ub_layout.addWidget(self.update_lbl, stretch=1)

        self.btn_update = QPushButton("Download & Restart")
        self.btn_update.setObjectName("btnPrimary")
        self.btn_update.clicked.connect(self._apply_update)
        ub_layout.addWidget(self.btn_update)
        main_layout.addWidget(self.update_banner)

        # 2. Top Header Bar
        header = QHBoxLayout()
        header.setSpacing(12)

        title_box = QVBoxLayout()
        title_box.setSpacing(2)
        h_title = QLabel("Meridian Device Hub")
        h_title.setObjectName("headerTitle")
        title_box.addWidget(h_title)

        mesh_ip = get_mesh_ip() or "mesh offline"
        self.mesh_lbl = QLabel(f"Tailscale Mesh: {mesh_ip}")
        self.mesh_lbl.setObjectName("headerSubtitle")
        title_box.addWidget(self.mesh_lbl)
        header.addLayout(title_box, stretch=1)

        # Global Control Buttons
        self.btn_start_all = QPushButton("Start All Ready")
        self.btn_start_all.setObjectName("btnPrimary")
        self.btn_start_all.clicked.connect(self._on_start_all_clicked)
        header.addWidget(self.btn_start_all)

        self.btn_stop_all = QPushButton("Stop All")
        self.btn_stop_all.setObjectName("btnStop")
        self.btn_stop_all.clicked.connect(self._on_stop_all_clicked)
        header.addWidget(self.btn_stop_all)

        main_layout.addLayout(header)

        # 3. Splitter between Device List and Console Logs
        splitter = QSplitter(Qt.Vertical)
        splitter.setHandleWidth(4)

        # Device Cards Scroll Area
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        self.cards_container = QWidget()
        self.cards_layout = QVBoxLayout(self.cards_container)
        self.cards_layout.setSpacing(12)
        self.cards_layout.setContentsMargins(0, 4, 0, 4)

        # Empty state prompt
        self.empty_lbl = QLabel(
            "No iPhones detected over USB.\n\n"
            "Connect an iPhone running iOS 27 with a verified USB cable to begin."
        )
        self.empty_lbl.setAlignment(Qt.AlignCenter)
        self.empty_lbl.setStyleSheet("color: #71717a; font-size: 14px; padding: 40px;")
        self.cards_layout.addWidget(self.empty_lbl)
        self.cards_layout.addStretch()

        scroll.setWidget(self.cards_container)
        splitter.addWidget(scroll)

        # Log Console
        self.log_console = LogConsoleWidget()
        splitter.addWidget(self.log_console)

        splitter.setSizes([420, 200])
        main_layout.addWidget(splitter, stretch=1)

    def _setup_orchestrator(self):
        self.device_changed_signal.connect(self._handle_device_changed)
        self.orchestrator.on_change = lambda report: self.device_changed_signal.emit(report)

        # Periodically refresh mesh IP display
        self.mesh_timer = QTimer(self)
        self.mesh_timer.timeout.connect(self._refresh_mesh_ip)
        self.mesh_timer.start(5000)

    def _setup_updater(self):
        self.update_available_signal.connect(self._show_update_banner)
        GLOBAL_UPDATER.on_update_available = lambda ver, url: self.update_available_signal.emit(ver, url)
        GLOBAL_UPDATER.check_for_updates_async()

    def _show_update_banner(self, ver: str, url: str):
        self.update_lbl.setText(f"🚀 Meridian v{ver} is available! (Current: v{CURRENT_VERSION})")
        self.update_banner.setVisible(True)

    def _apply_update(self):
        self.btn_update.setEnabled(False)
        self.update_lbl.setText("Downloading and staging update...")
        try:
            GLOBAL_UPDATER.download_update()
            self.update_lbl.setText("Applying update and restarting...")
            GLOBAL_UPDATER.apply_update_and_restart()
        except Exception as e:
            self.update_lbl.setText(f"Update failed: {e}")
            self.btn_update.setEnabled(True)

    def _refresh_mesh_ip(self):
        mesh_ip = get_mesh_ip()
        if mesh_ip:
            self.mesh_lbl.setText(f"Tailscale Mesh: {mesh_ip}")
        else:
            from ..remote.mesh import get_stored_authkey
            if get_stored_authkey():
                self.mesh_lbl.setText("Tailscale Mesh: connecting...")
            else:
                self.mesh_lbl.setText("Tailscale Mesh: no auth key (use --mesh-key or TS_AUTHKEY)")

    def _handle_device_changed(self, report: DeviceReport):
        udid = report.udid

        if report.state == DeviceState.DISCONNECTED:
            # Remove card
            card = self._cards.pop(udid, None)
            if card:
                self.cards_layout.removeWidget(card)
                card.deleteLater()
        else:
            # Update or create card
            if udid in self._cards:
                self._cards[udid].update_report(report)
            else:
                card = DeviceCard(report, parent=self.cards_container)
                card.start_requested.connect(self.orchestrator.start_device)
                card.stop_requested.connect(self.orchestrator.stop_device)
                card.refresh_requested.connect(self.orchestrator.reinspect_device)
                self._cards[udid] = card
                # Insert above the stretch item
                self.cards_layout.insertWidget(self.cards_layout.count() - 1, card)

        has_devices = len(self._cards) > 0
        self.empty_lbl.setVisible(not has_devices)

    def _on_start_all_clicked(self):
        count = self.orchestrator.start_all_ready()
        log.info("Start All: initiated %d ready devices", count)

    def _on_stop_all_clicked(self):
        self.orchestrator.stop_all()
        log.info("Stop All: all devices stopped")

    def closeEvent(self, event):
        log.info("window closing: stopping orchestrator...")
        self.orchestrator.stop()
        event.accept()
