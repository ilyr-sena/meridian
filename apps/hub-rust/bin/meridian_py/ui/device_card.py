"""
Visual Device Card Widget for the Meridian Desktop Hub.
Represents a connected iPhone with real-time status badges, allocated port slots,
and controls for streaming, onboarding, and sideloading.
"""
from __future__ import annotations

import logging
from typing import Callable, Optional

from PySide6.QtCore import Qt, Signal
from PySide6.QtWidgets import (
    QFrame,
    QHBoxLayout,
    QLabel,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

from ..core.inspector import DeviceReport, DeviceState
from ..core.slots import GLOBAL_SLOTS, DevicePorts
from .stepper import OnboardingDialog

log = logging.getLogger(__name__)


class DeviceCard(QFrame):
    """Interactive card widget for an individual iPhone."""

    start_requested = Signal(str)  # udid
    stop_requested = Signal(str)   # udid
    refresh_requested = Signal(str) # udid

    def __init__(self, report: DeviceReport, parent=None):
        super().__init__(parent)
        self.setObjectName("deviceCard")
        self.report = report
        self._init_ui()

    def _init_ui(self):
        layout = QVBoxLayout(self)
        layout.setSpacing(12)
        layout.setContentsMargins(16, 16, 16, 16)

        # Top row: Device Info + Status Badge
        top_row = QHBoxLayout()
        top_row.setSpacing(12)

        info_box = QVBoxLayout()
        info_box.setSpacing(2)

        self.name_lbl = QLabel(self.report.name)
        self.name_lbl.setObjectName("deviceName")
        info_box.addWidget(self.name_lbl)

        meta_str = f"{self.report.model} • {self.report.os_version} • {self.report.udid[-8:]}"
        self.meta_lbl = QLabel(meta_str)
        self.meta_lbl.setObjectName("deviceMeta")
        info_box.addWidget(self.meta_lbl)

        top_row.addLayout(info_box, stretch=1)

        self.badge_lbl = QLabel()
        top_row.addWidget(self.badge_lbl, alignment=Qt.AlignRight | Qt.AlignVCenter)
        layout.addLayout(top_row)

        # Middle row: Ports & Status message
        mid_row = QHBoxLayout()
        self.ports_lbl = QLabel()
        self.ports_lbl.setObjectName("portsLabel")
        mid_row.addWidget(self.ports_lbl)

        self.status_msg_lbl = QLabel(self.report.status_message)
        self.status_msg_lbl.setStyleSheet("color: #a1a1aa; font-size: 11px;")
        mid_row.addWidget(self.status_msg_lbl, stretch=1, alignment=Qt.AlignRight)
        layout.addLayout(mid_row)

        # Bottom row: Action Buttons
        bot_row = QHBoxLayout()
        bot_row.setSpacing(8)

        self.btn_toggle = QPushButton("Start")
        self.btn_toggle.setObjectName("btnPrimary")
        self.btn_toggle.clicked.connect(self._on_toggle_clicked)
        bot_row.addWidget(self.btn_toggle)

        self.btn_setup = QPushButton("Setup Guide")
        self.btn_setup.clicked.connect(self._on_setup_clicked)
        bot_row.addWidget(self.btn_setup)

        bot_row.addStretch()
        layout.addLayout(bot_row)

        self.update_report(self.report)

    def update_report(self, report: DeviceReport):
        self.report = report
        self.name_lbl.setText(report.name)
        self.meta_lbl.setText(f"{report.model} • {report.os_version} • {report.udid[-8:]}")
        self.status_msg_lbl.setText(report.status_message)

        # Update port slot
        ports = GLOBAL_SLOTS.get(report.udid)
        if ports:
            self.ports_lbl.setText(f"Slot #{ports.slot} [WDA:{ports.wda} • Bridge:{ports.bridge} • Stream:{ports.stream}]")
            self.ports_lbl.setVisible(True)
        else:
            self.ports_lbl.setVisible(False)

        # Update status badge cleanly with direct styling
        state = report.state
        if state == DeviceState.RUNNING:
            self.badge_lbl.setText("● ONLINE")
            self.badge_lbl.setStyleSheet("background-color: rgba(16, 185, 129, 0.2); color: #34d399; border: 1px solid rgba(16, 185, 129, 0.4); border-radius: 12px; padding: 4px 10px; font-size: 11px; font-weight: 600;")
            self.btn_toggle.setText("Stop")
            self.btn_toggle.setStyleSheet("background-color: rgba(239, 68, 68, 0.2); border: 1px solid rgba(239, 68, 68, 0.5); color: #f87171; font-weight: 600;")
            self.btn_toggle.setEnabled(True)
            self.btn_setup.setVisible(False)
        elif state == DeviceState.READY:
            self.badge_lbl.setText("✓ READY")
            self.badge_lbl.setStyleSheet("background-color: rgba(59, 130, 246, 0.2); color: #60a5fa; border: 1px solid rgba(59, 130, 246, 0.4); border-radius: 12px; padding: 4px 10px; font-size: 11px; font-weight: 600;")
            self.btn_toggle.setText("Start")
            self.btn_toggle.setStyleSheet("background-color: #7c3aed; border: 1px solid #8b5cf6; color: #ffffff; font-weight: 600;")
            self.btn_toggle.setEnabled(True)
            self.btn_setup.setVisible(False)
        elif state == DeviceState.UNPAIRED:
            self.badge_lbl.setText("⚠️ TRUST NEEDED")
            self.badge_lbl.setStyleSheet("background-color: rgba(245, 158, 11, 0.2); color: #fbbf24; border: 1px solid rgba(245, 158, 11, 0.4); border-radius: 12px; padding: 4px 10px; font-size: 11px; font-weight: 600;")
            self.btn_toggle.setText("Setup Needed")
            self.btn_toggle.setStyleSheet("")
            self.btn_toggle.setEnabled(False)
            self.btn_setup.setVisible(True)
        elif state == DeviceState.LOCKED:
            self.badge_lbl.setText("🔒 LOCKED")
            self.badge_lbl.setStyleSheet("background-color: rgba(239, 68, 68, 0.2); color: #f87171; border: 1px solid rgba(239, 68, 68, 0.4); border-radius: 12px; padding: 4px 10px; font-size: 11px; font-weight: 600;")
            self.btn_toggle.setText("Unlock First")
            self.btn_toggle.setStyleSheet("")
            self.btn_toggle.setEnabled(False)
            self.btn_setup.setVisible(True)
        elif state in (DeviceState.DEVMODE_DISABLED, DeviceState.DEVMODE_HIDDEN):
            self.badge_lbl.setText("🛠 DEV MODE")
            self.badge_lbl.setStyleSheet("background-color: rgba(245, 158, 11, 0.2); color: #fbbf24; border: 1px solid rgba(245, 158, 11, 0.4); border-radius: 12px; padding: 4px 10px; font-size: 11px; font-weight: 600;")
            self.btn_toggle.setText("Enable Dev Mode")
            self.btn_toggle.setStyleSheet("background-color: #27272a; border: 1px solid #3f3f46; color: #f4f4f5; font-weight: 600;")
            self.btn_toggle.setEnabled(True)
            self.btn_setup.setVisible(True)
        elif state == DeviceState.NO_RUNNER_APP:
            self.badge_lbl.setText("📦 NO RUNNER")
            self.badge_lbl.setStyleSheet("background-color: rgba(139, 92, 246, 0.2); color: #a78bfa; border: 1px solid rgba(139, 92, 246, 0.4); border-radius: 12px; padding: 4px 10px; font-size: 11px; font-weight: 600;")
            self.btn_toggle.setText("Sideload Runner")
            self.btn_toggle.setStyleSheet("background-color: #7c3aed; border: 1px solid #8b5cf6; color: #ffffff; font-weight: 600;")
            self.btn_toggle.setEnabled(True)
            self.btn_setup.setVisible(True)
        elif state == DeviceState.UNSUPPORTED_IOS:
            self.badge_lbl.setText("❌ UNSUPPORTED")
            self.badge_lbl.setStyleSheet("background-color: rgba(239, 68, 68, 0.2); color: #f87171; border: 1px solid rgba(239, 68, 68, 0.4); border-radius: 12px; padding: 4px 10px; font-size: 11px; font-weight: 600;")
            self.btn_toggle.setText("iOS 27 Only")
            self.btn_toggle.setStyleSheet("")
            self.btn_toggle.setEnabled(False)
            self.btn_setup.setVisible(False)
        else:
            self.badge_lbl.setText("OFFLINE")
            self.badge_lbl.setStyleSheet("background-color: #27272a; color: #a1a1aa; border-radius: 12px; padding: 4px 10px; font-size: 11px;")
            self.btn_toggle.setStyleSheet("")
            self.btn_toggle.setEnabled(False)
            self.btn_setup.setVisible(False)

    def _on_toggle_clicked(self):
        if self.report.state == DeviceState.RUNNING:
            self.stop_requested.emit(self.report.udid)
        elif self.report.state == DeviceState.NO_RUNNER_APP:
            from .sideload_dialog import SideloadDialog
            dlg = SideloadDialog(self.report.udid, parent=self)
            if dlg.exec():
                self.refresh_requested.emit(self.report.udid)
        elif self.report.state in (DeviceState.DEVMODE_DISABLED, DeviceState.DEVMODE_HIDDEN):
            self._on_setup_clicked()
        else:
            self.start_requested.emit(self.report.udid)

    def _on_setup_clicked(self):
        dlg = OnboardingDialog(self.report, parent=self)
        if dlg.exec():
            self.refresh_requested.emit(self.report.udid)
