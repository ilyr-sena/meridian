"""
Live High-Performance Activity Log Console for Meridian Desktop.
Integrates directly with Python's logging module to stream logs smoothly
without UI lag or visual glitching. Uses a throttled queue + timer drain.
"""
from __future__ import annotations

import logging
import pathlib
import queue
from datetime import datetime
from typing import Optional

from PySide6.QtCore import Qt, QTimer
from PySide6.QtGui import QTextCursor
from PySide6.QtWidgets import (
    QFileDialog,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPlainTextEdit,
    QPushButton,
    QVBoxLayout,
    QWidget,
)

LOG_QUEUE: queue.SimpleQueue = queue.SimpleQueue()


class ThrottledQtLogHandler(logging.Handler):
    """Thread-safe logging handler that puts records into a memory queue."""

    def emit(self, record: logging.LogRecord):
        try:
            # Drop high-frequency coordinate/jitter logs from console
            msg = record.getMessage()
            if "Tap received" in msg or "jitter" in msg:
                return
            formatted = self.format(record)
            LOG_QUEUE.put_nowait(formatted)
        except Exception:
            pass


class LogConsoleWidget(QWidget):
    """Interactive log console dock widget updated at smooth 10Hz batches."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self._init_ui()
        self._setup_logging()
        self._setup_timer()

    def _init_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(8)

        # Toolbar: Title + Filter + Clear + Export
        bar = QHBoxLayout()
        bar.setSpacing(8)

        title = QLabel("Hardware & Activity Logs")
        title.setStyleSheet("font-weight: 600; font-size: 12px; color: #a1a1aa;")
        bar.addWidget(title)

        bar.addStretch()

        self.filter_input = QLineEdit()
        self.filter_input.setPlaceholderText("Filter logs...")
        self.filter_input.setFixedWidth(160)
        bar.addWidget(self.filter_input)

        self.btn_clear = QPushButton("Clear")
        self.btn_clear.clicked.connect(self._clear_logs)
        bar.addWidget(self.btn_clear)

        self.btn_export = QPushButton("Export")
        self.btn_export.clicked.connect(self._export_logs)
        bar.addWidget(self.btn_export)

        layout.addLayout(bar)

        # Text Console
        self.console = QPlainTextEdit()
        self.console.setObjectName("logConsole")
        self.console.setReadOnly(True)
        self.console.setMaximumBlockCount(3000)
        layout.addWidget(self.console)

    def _setup_logging(self):
        formatter = logging.Formatter("%(asctime)s [%(levelname)s] %(name)s: %(message)s", datefmt="%H:%M:%S")
        self.handler = ThrottledQtLogHandler()
        self.handler.setFormatter(formatter)
        logging.getLogger().addHandler(self.handler)

    def _setup_timer(self):
        # 10Hz batch update: drains queue without stalling GUI thread
        self.timer = QTimer(self)
        self.timer.timeout.connect(self._drain_queue)
        self.timer.start(100)

    def _drain_queue(self):
        batch = []
        filter_text = self.filter_input.text().strip().lower()

        while not LOG_QUEUE.empty() and len(batch) < 100:
            try:
                line = LOG_QUEUE.get_nowait()
                if filter_text and filter_text not in line.lower():
                    continue
                batch.append(line)
            except queue.Empty:
                break

        if batch:
            self.console.appendPlainText("\n".join(batch))
            self.console.moveCursor(QTextCursor.End)

    def _clear_logs(self):
        self.console.clear()

    def _export_logs(self):
        ts = datetime.now().strftime("%Y%m%d_%H%M%S")
        path, _ = QFileDialog.getSaveFileName(
            self, "Export Activity Logs", str(pathlib.Path.home() / f"meridian_log_{ts}.txt"), "Text Files (*.txt)"
        )
        if path:
            pathlib.Path(path).write_text(self.console.toPlainText(), encoding="utf-8")
