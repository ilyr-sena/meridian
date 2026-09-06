"""
Interactive Onboarding Stepper Dialog for Meridian.
Guides the user cleanly through every step or combination to make the phone Meridian-ready:
  1. USB Connection
  2. Trust Pairing
  3. Passcode Unlock
  4. iOS 27 Gate
  5. Developer Mode (with AMFI reveal & reboot trigger)
  6. MeridianRunner App Presence & Sideload
"""
from __future__ import annotations

import asyncio
import logging
from typing import Optional

from PySide6.QtCore import Qt, QThread, Signal
from PySide6.QtWidgets import (
    QDialog,
    QFormLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMessageBox,
    QProgressBar,
    QPushButton,
    QVBoxLayout,
)

from ..core.inspector import DeviceInspector, DeviceReport, DeviceState

log = logging.getLogger(__name__)


class AsyncWorkerThread(QThread):
    finished_signal = Signal(bool, object)

    def __init__(self, coro_func, *args, **kwargs):
        super().__init__()
        self.coro_func = coro_func
        self.args = args
        self.kwargs = kwargs

    def run(self):
        try:
            res = asyncio.run(self.coro_func(*self.args, **self.kwargs))
            self.finished_signal.emit(True, res)
        except Exception as e:
            self.finished_signal.emit(False, str(e))


class OnboardingDialog(QDialog):
    """Step-by-step interactive setup wizard for a connected device."""

    def __init__(self, report: DeviceReport, parent=None):
        super().__init__(parent)
        self.report = report
        self.setWindowTitle(f"Device Setup — {report.name} ({report.udid[-6:]})")
        self.resize(520, 440)
        self._init_ui()

    def _init_ui(self):
        layout = QVBoxLayout(self)
        layout.setSpacing(16)
        layout.setContentsMargins(24, 24, 24, 24)

        # Title & Subtitle
        header = QLabel(f"Meridian Onboarding: {self.report.name}")
        header.setObjectName("headerTitle")
        layout.addWidget(header)

        sub = QLabel(f"UDID: {self.report.udid} • Model: {self.report.model} • OS: {self.report.os_version}")
        sub.setObjectName("headerSubtitle")
        layout.addWidget(sub)

        # Status Banner
        self.status_banner = QLabel(self._get_state_headline())
        self.status_banner.setWordWrap(True)
        self.status_banner.setStyleSheet(self._get_badge_style())
        layout.addWidget(self.status_banner)

        # Instructions / Action Box
        self.action_label = QLabel(self.report.user_action_step or "Evaluating requirements...")
        self.action_label.setWordWrap(True)
        self.action_label.setStyleSheet("background-color: #18181b; border: 1px solid #27272a; border-radius: 8px; padding: 12px; font-size: 13px; line-height: 1.4;")
        layout.addWidget(self.action_label)

        # Progress bar
        self.progress = QProgressBar()
        self.progress.setTextVisible(False)
        self.progress.setStyleSheet("QProgressBar { background-color: #27272a; border-radius: 4px; height: 6px; } QProgressBar::chunk { background-color: #7c3aed; border-radius: 4px; }")
        layout.addWidget(self.progress)
        self._update_progress_bar()

        # Dynamic Action Buttons
        self.btn_layout = QHBoxLayout()
        self.btn_layout.setSpacing(10)

        self.btn_action = QPushButton("Resolve Step")
        self.btn_action.setObjectName("btnPrimary")
        self.btn_action.clicked.connect(self._on_action_clicked)
        self.btn_layout.addWidget(self.btn_action)

        self.btn_refresh = QPushButton("Re-check Status")
        self.btn_refresh.clicked.connect(self._on_refresh_clicked)
        self.btn_layout.addWidget(self.btn_refresh)

        self.btn_close = QPushButton("Close")
        self.btn_close.clicked.connect(self.close)
        self.btn_layout.addWidget(self.btn_close)

        layout.addLayout(self.btn_layout)
        self._update_action_button()

    def _get_state_headline(self) -> str:
        s = self.report.state
        if s == DeviceState.READY:
            return "✓ Device is fully configured and ready to connect!"
        if s == DeviceState.UNPAIRED:
            return "⚠️ Pairing required: Tap 'Trust' on iPhone"
        if s == DeviceState.LOCKED:
            return "🔒 Passcode lock active: Unlock device"
        if s == DeviceState.UNSUPPORTED_IOS:
            return f"❌ Incompatible OS: Requires iOS 27.x (Found {self.report.os_version})"
        if s in (DeviceState.DEVMODE_DISABLED, DeviceState.DEVMODE_HIDDEN):
            return "🛠 Developer Mode required"
        if s == DeviceState.NO_RUNNER_APP:
            return "📦 MeridianRunner app not sideloaded"
        return f"Status: {self.report.status_message}"

    def _get_badge_style(self) -> str:
        if self.report.state == DeviceState.READY:
            return "background-color: rgba(16, 185, 129, 0.2); color: #34d399; border: 1px solid rgba(16, 185, 129, 0.4); border-radius: 8px; padding: 10px; font-weight: 600;"
        if self.report.state in (DeviceState.LOCKED, DeviceState.UNSUPPORTED_IOS):
            return "background-color: rgba(239, 68, 68, 0.2); color: #f87171; border: 1px solid rgba(239, 68, 68, 0.4); border-radius: 8px; padding: 10px; font-weight: 600;"
        return "background-color: rgba(245, 158, 11, 0.2); color: #fbbf24; border: 1px solid rgba(245, 158, 11, 0.4); border-radius: 8px; padding: 10px; font-weight: 600;"

    def _update_progress_bar(self):
        order = [
            DeviceState.DISCONNECTED,
            DeviceState.UNPAIRED,
            DeviceState.LOCKED,
            DeviceState.UNSUPPORTED_IOS,
            DeviceState.DEVMODE_HIDDEN,
            DeviceState.DEVMODE_DISABLED,
            DeviceState.NO_RUNNER_APP,
            DeviceState.READY,
        ]
        try:
            idx = order.index(self.report.state)
            pct = int((idx / (len(order) - 1)) * 100)
        except ValueError:
            pct = 100 if self.report.is_operational() else 30
        self.progress.setValue(pct)

    def _update_action_button(self):
        s = self.report.state
        if s == DeviceState.READY:
            self.btn_action.setText("Finish Setup")
            self.btn_action.setEnabled(True)
        elif s == DeviceState.DEVMODE_HIDDEN:
            self.btn_action.setText("Reveal Developer Mode")
            self.btn_action.setEnabled(True)
        elif s == DeviceState.DEVMODE_DISABLED:
            self.btn_action.setText("Trigger Activation & Reboot")
            self.btn_action.setEnabled(True)
        elif s == DeviceState.NO_RUNNER_APP:
            self.btn_action.setText("Sideload MeridianRunner")
            self.btn_action.setEnabled(True)
        else:
            self.btn_action.setText("Re-Check")

    def _on_action_clicked(self):
        s = self.report.state
        if s == DeviceState.READY:
            self.accept()
        elif s == DeviceState.DEVMODE_HIDDEN:
            self._reveal_dev_mode()
        elif s == DeviceState.DEVMODE_DISABLED:
            self._trigger_dev_mode_reboot()
        elif s == DeviceState.NO_RUNNER_APP:
            self._sideload_runner()
        else:
            self._on_refresh_clicked()

    def _reveal_dev_mode(self):
        self.btn_action.setEnabled(False)
        self.action_label.setText("Triggering AMFI service to reveal Developer Mode option...")
        self.worker = AsyncWorkerThread(DeviceInspector.reveal_developer_mode, self.report.udid)
        self.worker.finished_signal.connect(self._on_reveal_done)
        self.worker.start()

    def _on_reveal_done(self, ok: bool, msg: str):
        if ok:
            QMessageBox.information(
                self,
                "Developer Mode Revealed",
                "Developer Mode toggle is now visible in your iPhone Settings!\n\n"
                "Go to Settings → Privacy & Security → Developer Mode, turn it ON, and restart.",
            )
        else:
            QMessageBox.warning(self, "Reveal Failed", f"Could not reveal Developer Mode automatically: {msg}")
        self._on_refresh_clicked()

    def _trigger_dev_mode_reboot(self):
        reply = QMessageBox.question(
            self,
            "Enable Developer Mode & Reboot",
            "This will instruct your iPhone to enable Developer Mode and reboot now.\n"
            "Upon restarting, tap 'Turn On' when prompted on your device.\n\nProceed?",
            QMessageBox.Yes | QMessageBox.No,
        )
        if reply != QMessageBox.Yes:
            return
        self.btn_action.setEnabled(False)
        self.action_label.setText("Scheduling Developer Mode enable and reboot...")
        self.worker = AsyncWorkerThread(DeviceInspector.trigger_developer_mode_enable, self.report.udid)
        self.worker.finished_signal.connect(self._on_reboot_triggered)
        self.worker.start()

    def _on_reboot_triggered(self, ok: bool, msg: str):
        if ok:
            QMessageBox.information(self, "Reboot Scheduled", "Device is rebooting. Keep the USB cable plugged in.")
        else:
            QMessageBox.warning(self, "Enable Error", f"Could not automatically trigger reboot: {msg}")
        self._on_refresh_clicked()

    def _sideload_runner(self):
        from .sideload_dialog import SideloadDialog
        dlg = SideloadDialog(self.report.udid, parent=self)
        if dlg.exec():
            self._on_refresh_clicked()

    def _on_refresh_clicked(self):
        self.action_label.setText("Refreshing device status...")
        async def _check():
            return await DeviceInspector.inspect(self.report.udid)
        self.refresh_worker = AsyncWorkerThread(_check)
        self.refresh_worker.finished_signal.connect(self._on_report_updated)
        self.refresh_worker.start()

    def _on_report_updated(self, ok: bool, result: object):
        if ok and isinstance(result, DeviceReport):
            self.report = result
            self.status_banner.setText(self._get_state_headline())
            self.status_banner.setStyleSheet(self._get_badge_style())
            self.action_label.setText(self.report.user_action_step or self.report.status_message)
            self._update_progress_bar()
            self._update_action_button()
        elif not ok:
            self.action_label.setText(f"Refresh error: {result}")
