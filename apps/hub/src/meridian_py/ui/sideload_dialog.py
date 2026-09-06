"""
Apple ID Sign-in and Automated Sideload Dialog for Meridian.
Uses the exact, working Apple authentication and sideloading pipeline from meridian_py.sideload.
"""
from __future__ import annotations

import logging
import os
import pathlib
import sys
import threading
from typing import Optional

from PySide6.QtCore import Qt, QThread, Signal
from PySide6.QtWidgets import (
    QCheckBox,
    QDialog,
    QFileDialog,
    QFormLayout,
    QHBoxLayout,
    QInputDialog,
    QLabel,
    QLineEdit,
    QMessageBox,
    QProgressBar,
    QPushButton,
    QVBoxLayout,
)

from ..core.vault import GLOBAL_VAULT, get_meridian_cache_dir
from ..sideload import sideload_app

log = logging.getLogger(__name__)

DEFAULT_IPA_PATHS = [
    get_meridian_cache_dir() / "MeridianRunner-unsigned.ipa",
]


class SideloadWorkerThread(QThread):
    finished_signal = Signal(bool, str)
    log_signal = Signal(str)
    code_request_signal = Signal(str)
    code_response = Signal(str)

    def __init__(self, ipa_path: pathlib.Path, udid: str, apple_id: str, password: str, anisette_url: str = ""):
        super().__init__()
        self.ipa_path = ipa_path
        self.udid = udid
        self.apple_id = apple_id
        self.password = password
        self.anisette_url = anisette_url
        self._code = None
        self._code_event = threading.Event()

    def _request_code(self, prompt: str) -> str:
        self._code = None
        self._code_event.clear()
        self.code_request_signal.emit(prompt)
        self._code_event.wait(timeout=120)
        return self._code or ""

    def provide_code(self, code: str):
        self._code = code
        self._code_event.set()

    def run(self):
        try:
            self.log_signal.emit(f"Starting sideload of {self.ipa_path.name} to {self.udid[-6:]}...")
            sideload_app(
                ipa_path=self.ipa_path,
                udid=self.udid,
                apple_id=self.apple_id or None,
                password=self.password or None,
                use_browser=False,
                anisette_url=self.anisette_url,
                code_callback=self._request_code,
            )
            self.finished_signal.emit(True, "MeridianRunner signed and installed successfully!")
        except Exception as e:
            self.finished_signal.emit(False, str(e))


class SideloadDialog(QDialog):
    """Secure Apple ID credential prompt and automated sideload runner."""

    def __init__(self, udid: str, parent=None):
        super().__init__(parent)
        self.udid = udid
        self.setWindowTitle("Sideload MeridianRunner — Apple ID")
        self.resize(460, 320)
        self._init_ui()

    def _init_ui(self):
        layout = QVBoxLayout(self)
        layout.setSpacing(14)
        layout.setContentsMargins(20, 20, 20, 20)

        header = QLabel("Apple Developer Sign-In")
        header.setObjectName("headerTitle")
        layout.addWidget(header)

        sub = QLabel(
            "Enter your Apple ID to sign and install the MeridianRunner app on your device. "
            "Credentials are sent directly to Apple's GSA server and stored in your encrypted local vault."
        )
        sub.setObjectName("headerSubtitle")
        sub.setWordWrap(True)
        layout.addWidget(sub)

        form = QFormLayout()
        form.setSpacing(10)

        saved_id, saved_pwd = GLOBAL_VAULT.get_apple_credentials()

        self.input_email = QLineEdit()
        self.input_email.setPlaceholderText("name@example.com")
        if saved_id:
            self.input_email.setText(saved_id)
        form.addRow("Apple ID:", self.input_email)

        self.input_pwd = QLineEdit()
        self.input_pwd.setEchoMode(QLineEdit.Password)
        self.input_pwd.setPlaceholderText("Password")
        if saved_pwd:
            self.input_pwd.setText(saved_pwd)
        form.addRow("Password:", self.input_pwd)

        self.input_ipa = QLineEdit()
        default_ipa = self._find_default_ipa()
        if default_ipa:
            self.input_ipa.setText(str(default_ipa))
        form.addRow("Runner IPA:", self.input_ipa)

        self.input_anisette = QLineEdit()
        self.input_anisette.setPlaceholderText("http://vps-ip:6969 (leave empty for local)")
        saved_anisette = GLOBAL_VAULT.get_anisette_url()
        if saved_anisette:
            self.input_anisette.setText(saved_anisette)
        form.addRow("Anisette URL:", self.input_anisette)

        layout.addLayout(form)

        self.check_save = QCheckBox("Save credentials securely in encrypted vault")
        self.check_save.setChecked(True)
        layout.addWidget(self.check_save)

        self.progress = QProgressBar()
        self.progress.setTextVisible(False)
        self.progress.setVisible(False)
        self.progress.setStyleSheet("QProgressBar { background-color: #27272a; height: 6px; border-radius: 3px; } QProgressBar::chunk { background-color: #7c3aed; }")
        layout.addWidget(self.progress)

        self.status_lbl = QLabel("")
        self.status_lbl.setStyleSheet("color: #a1a1aa; font-size: 11px;")
        layout.addWidget(self.status_lbl)

        btn_layout = QHBoxLayout()
        self.btn_submit = QPushButton("Sign & Sideload")
        self.btn_submit.setObjectName("btnPrimary")
        self.btn_submit.clicked.connect(self._start_sideload)
        btn_layout.addWidget(self.btn_submit)

        self.btn_cancel = QPushButton("Cancel")
        self.btn_cancel.clicked.connect(self.reject)
        btn_layout.addWidget(self.btn_cancel)

        layout.addLayout(btn_layout)

    def _find_default_ipa(self) -> Optional[pathlib.Path]:
        for p in DEFAULT_IPA_PATHS:
            if p.exists():
                return p
        return None

    def _start_sideload(self):
        email = self.input_email.text().strip()
        pwd = self.input_pwd.text().strip()
        ipa_str = self.input_ipa.text().strip()
        anisette_url = self.input_anisette.text().strip()

        if not email or not pwd:
            QMessageBox.warning(self, "Missing Credentials", "Please enter your Apple ID and password.")
            return

        ipa_path = pathlib.Path(ipa_str)
        if not ipa_path.exists():
            QMessageBox.warning(self, "Missing IPA", f"IPA file not found: {ipa_str}")
            return

        if self.check_save.isChecked():
            GLOBAL_VAULT.set_apple_credentials(email, pwd)

        if anisette_url:
            GLOBAL_VAULT.set_anisette_url(anisette_url)

        self.btn_submit.setEnabled(False)
        self.progress.setVisible(True)
        self.progress.setRange(0, 0) # Indeterminate spinning
        self.status_lbl.setText("Authenticating with Apple GSA & signing frameworks...")

        self.worker = SideloadWorkerThread(ipa_path, self.udid, email, pwd, anisette_url=anisette_url)
        self.worker.log_signal.connect(self.status_lbl.setText)
        self.worker.finished_signal.connect(self._on_finished)
        self.worker.code_request_signal.connect(self._on_code_request)
        self.worker.start()

    def _on_code_request(self, prompt: str):
        code, ok = QInputDialog.getText(self, "Two-Factor Authentication", prompt)
        if ok and code:
            self.worker.provide_code(code.strip())

    def _on_finished(self, ok: bool, msg: str):
        self.progress.setVisible(False)
        self.btn_submit.setEnabled(True)
        if ok:
            QMessageBox.information(self, "Sideload Complete", "MeridianRunner was successfully signed and installed!")
            self.accept()
        else:
            QMessageBox.critical(self, "Sideload Failed", f"Sideload error:\n\n{msg}")
            self.status_lbl.setText(f"Error: {msg}")
