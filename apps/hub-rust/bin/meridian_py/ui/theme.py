"""
Dark modern stylesheet for the Meridian desktop application.
Matches the Next.js / Tailwind zinc-slate dark aesthetic.
"""
from __future__ import annotations

DARK_STYLE = """
QMainWindow, QDialog {
    background-color: #09090b;
    color: #f4f4f5;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
    font-size: 13px;
}

QWidget {
    background-color: transparent;
    color: #f4f4f5;
}

QScrollArea {
    border: none;
    background-color: transparent;
}

QScrollBar:vertical {
    border: none;
    background: #18181b;
    width: 8px;
    margin: 0px 0px 0px 0px;
    border-radius: 4px;
}

QScrollBar::handle:vertical {
    background: #3f3f46;
    min-height: 20px;
    border-radius: 4px;
}

QScrollBar::handle:vertical:hover {
    background: #71717a;
}

QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical {
    height: 0px;
}

/* Card Styling */
QFrame#deviceCard {
    background-color: #18181b;
    border: 1px solid #27272a;
    border-radius: 16px;
    padding: 16px;
}

QFrame#deviceCard:hover {
    border: 1px solid #3f3f46;
}

/* Header & Typography */
QLabel#headerTitle {
    font-size: 18px;
    font-weight: 700;
    color: #fafafa;
}

QLabel#headerSubtitle {
    font-size: 12px;
    color: #a1a1aa;
}

QLabel#deviceName {
    font-size: 15px;
    font-weight: 600;
    color: #f4f4f5;
}

QLabel#deviceMeta {
    font-size: 12px;
    color: #a1a1aa;
}

QLabel#portsLabel {
    font-family: monospace;
    font-size: 11px;
    color: #a1a1aa;
    background-color: #27272a;
    border-radius: 6px;
    padding: 3px 8px;
}

/* Status Badges */
QLabel#badgeOnline {
    background-color: rgba(16, 185, 129, 0.15);
    color: #34d399;
    border: 1px solid rgba(16, 185, 129, 0.3);
    border-radius: 12px;
    padding: 4px 10px;
    font-size: 11px;
    font-weight: 600;
}

QLabel#badgeReady {
    background-color: rgba(59, 130, 246, 0.15);
    color: #60a5fa;
    border: 1px solid rgba(59, 130, 246, 0.3);
    border-radius: 12px;
    padding: 4px 10px;
    font-size: 11px;
    font-weight: 600;
}

QLabel#badgeWarning {
    background-color: rgba(245, 158, 11, 0.15);
    color: #fbbf24;
    border: 1px solid rgba(245, 158, 11, 0.3);
    border-radius: 12px;
    padding: 4px 10px;
    font-size: 11px;
    font-weight: 600;
}

QLabel#badgeDanger {
    background-color: rgba(239, 68, 68, 0.15);
    color: #f87171;
    border: 1px solid rgba(239, 68, 68, 0.3);
    border-radius: 12px;
    padding: 4px 10px;
    font-size: 11px;
    font-weight: 600;
}

/* Primary Action Buttons */
QPushButton {
    background-color: #27272a;
    border: 1px solid #3f3f46;
    color: #f4f4f5;
    border-radius: 8px;
    padding: 7px 14px;
    font-weight: 500;
}

QPushButton:hover {
    background-color: #3f3f46;
    border: 1px solid #52525b;
}

QPushButton:pressed {
    background-color: #18181b;
}

QPushButton#btnPrimary {
    background-color: #7c3aed;
    border: 1px solid #8b5cf6;
    color: #ffffff;
    font-weight: 600;
}

QPushButton#btnPrimary:hover {
    background-color: #6d28d9;
    border: 1px solid #7c3aed;
}

QPushButton#btnPrimary:disabled {
    background-color: #3f3f46;
    border: 1px solid #27272a;
    color: #71717a;
}

QPushButton#btnStop {
    background-color: rgba(239, 68, 68, 0.15);
    border: 1px solid rgba(239, 68, 68, 0.4);
    color: #f87171;
    font-weight: 600;
}

QPushButton#btnStop:hover {
    background-color: rgba(239, 68, 68, 0.25);
    border: 1px solid rgba(239, 68, 68, 0.6);
}

/* Console Log View */
QPlainTextEdit#logConsole {
    background-color: #0f0f11;
    border: 1px solid #27272a;
    border-radius: 8px;
    color: #d4d4d8;
    font-family: "JetBrains Mono", Menlo, Monaco, Consolas, "Courier New", monospace;
    font-size: 11px;
    padding: 8px;
}

QLineEdit {
    background-color: #18181b;
    border: 1px solid #3f3f46;
    border-radius: 8px;
    color: #f4f4f5;
    padding: 8px 12px;
}

QLineEdit:focus {
    border: 1px solid #8b5cf6;
}
"""
