# -*- mode: python ; coding: utf-8 -*-
"""
PyInstaller Single-Executable Build Specification for Meridian.
Compiles Meridian into a self-contained, all-in-one desktop app & daemon
for Linux and Windows without requiring system Python or external tools.
"""
import os
import pathlib
import sys

from PyInstaller.utils.hooks import collect_all, collect_data_files, collect_submodules

block_cipher = None

# Base directories
root_dir = pathlib.Path.cwd().resolve()
src_dir = root_dir / "src"

# Locate meridian-mesh Go binary to embed
mesh_binary = None
mesh_name = "meridian-mesh.exe" if sys.platform == "win32" else "meridian-mesh"
for candidate in [
    root_dir / "bin" / mesh_name,
    pathlib.Path.home() / ".local" / "bin" / mesh_name,
    root_dir.parent / "meridian-mesh" / mesh_name,
]:
    if candidate.exists():
        mesh_binary = str(candidate)
        break

# Locate zsign binary to embed
zsign_binary = None
zsign_name = "zsign.exe" if sys.platform == "win32" else "zsign"
for candidate in [
    root_dir / "bin" / zsign_name,
    pathlib.Path.home() / ".local" / "bin" / zsign_name,
    pathlib.Path(os.environ.get("LOCALAPPDATA", "")) / "Meridian" / "bin" / zsign_name,
]:
    if candidate.exists():
        zsign_binary = str(candidate)
        break

datas = []
binaries = []

if mesh_binary:
    binaries.append((mesh_binary, "."))
if zsign_binary:
    binaries.append((zsign_binary, "."))

# Collect pymobiledevice3 data and dependencies
try:
    pmd3_datas, pmd3_bins, pmd3_hidden = collect_all("pymobiledevice3")
    datas += pmd3_datas
    binaries += pmd3_bins
except Exception:
    pmd3_hidden = []

try:
    pmd3_submodules = collect_submodules("pymobiledevice3")
except Exception:
    pmd3_submodules = []

try:
    pyside6_datas = collect_data_files("PySide6")
    pyside6_datas = [d for d in pyside6_datas if not any(x in str(d[0]).lower() for x in ["qml", "designer", "quick", "translations"])]
    datas += pyside6_datas
except Exception:
    pass

hiddenimports = [
    "meridian_py",
    "meridian_py.cli",
    "meridian_py.commands",
    "meridian_py.config",
    "meridian_py.log",
    "meridian_py.models",
    "meridian_py.runner",
    "meridian_py.device_bridge",
    "meridian_py.core",
    "meridian_py.core.inspector",
    "meridian_py.core.slots",
    "meridian_py.core.vault",
    "meridian_py.core.updater",
    "meridian_py.core.firewall",
    "meridian_py.remote",
    "meridian_py.remote.bridge",
    "meridian_py.remote.heartbeat",
    "meridian_py.remote.lifecycle",
    "meridian_py.remote.mesh",
    "meridian_py.remote.orchestrator",
    "meridian_py.ui",
    "meridian_py.ui.app",
    "meridian_py.ui.main_window",
    "meridian_py.ui.device_card",
    "meridian_py.ui.stepper",
    "meridian_py.ui.sideload_dialog",
    "meridian_py.ui.log_console",
    "meridian_py.ui.theme",
    "meridian_py.sideload",
    "meridian_py.sideload.anisette_server",
    "meridian_py.sideload.ca",
    "meridian_py.sideload.gsa",
    "meridian_py.sideload.installer",
    "meridian_py.sideload.login",
    "meridian_py.sideload.profile",
    "meridian_py.sideload.provision",
    "meridian_py.sideload.signer",
    "meridian_py.devices",
    "meridian_py.devices.watcher",
    "meridian_py.devices.registry",
    "meridian_py.mux",
    "meridian_py.mux.tunnel",
    "meridian_py.mux.client",
    "meridian_py.mux.lockdown",
    "meridian_py.services",
    "meridian_py.services.supervisor",
    "meridian_py.services.tunneld",
    "meridian_py.services.iproxy",
    "meridian_py.stream",
    "meridian_py.stream.h264",
    "meridian_py.stream.wda",
    "meridian_py.server",
    "meridian_py.server.http",
    "PySide6.QtCore",
    "PySide6.QtGui",
    "PySide6.QtWidgets",
    "cryptography",
    "cryptography.hazmat.primitives.ciphers",
    "cryptography.hazmat.primitives.kdf.pbkdf2",
    "cryptography.hazmat.primitives.asymmetric.rsa",
    "cryptography.x509",
    "requests",
    "srp",
    "plistlib",
] + pmd3_hidden + pmd3_submodules

a = Analysis(
    [str(root_dir / "packaging" / "entrypoint.py")],
    pathex=[str(src_dir)],
    binaries=binaries,
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    win_no_prefer_redirects=False,
    win_private_assemblies=False,
    cipher=block_cipher,
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.zipfiles,
    a.datas,
    [],
    name="meridian" if sys.platform != "win32" else "meridian.exe",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)

