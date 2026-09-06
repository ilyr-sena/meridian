#!/usr/bin/env python3
"""
Automated Build Script for compiling Meridian into a single standalone binary.

Usage:
    python packaging/build_binary.py
    python packaging/build_binary.py --sign          # Sign with self-signed cert
    python packaging/build_binary.py --sign --pfx PATH  # Sign with custom PFX

Environment variables for code signing:
    MERIDIAN_PFX_PATH    Path to .pfx certificate file
    MERIDIAN_PFX_PASS    Password for the .pfx file
"""
import argparse
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
SPEC = ROOT / "packaging" / "meridian.spec"
DIST = ROOT / "dist"
CERT_DIR = ROOT / "packaging" / "certs"


def _find_signtool() -> str | None:
    """Locate signtool.exe from Windows SDK."""
    # Common SDK paths
    for base in [
        r"C:\Program Files (x86)\Windows Kits\10\bin",
        r"C:\Program Files\Windows Kits\10\bin",
    ]:
        base_path = pathlib.Path(base)
        if base_path.exists():
            # Find latest version
            versions = sorted(base_path.iterdir(), reverse=True)
            for ver in versions:
                signtool = ver / "x64" / "signtool.exe"
                if signtool.exists():
                    return str(signtool)
                signtool = ver / "x86" / "signtool.exe"
                if signtool.exists():
                    return str(signtool)
    # Try PATH
    import shutil
    return shutil.which("signtool.exe")


def _create_self_signed_cert() -> pathlib.Path:
    """Create a self-signed code signing certificate for development."""
    CERT_DIR.mkdir(parents=True, exist_ok=True)
    pfx_path = CERT_DIR / "meridian-dev.pfx"

    if pfx_path.exists():
        print(f"  Using existing self-signed cert: {pfx_path}")
        return pfx_path

    print("  Creating self-signed development certificate...")
    # Create cert via PowerShell (New-SelfSignedCertificate)
    ps_cmd = (
        f'$cert = New-SelfSignedCertificate '
        f'-Type CodeSigningCert '
        f'-Subject "CN=Meridian Development" '
        f'-CertStoreLocation Cert:\\CurrentUser\\My '
        f'-NotAfter (Get-Date).AddYears(5) '
        f'-HashAlgorithm SHA256; '
        f'$pwd = ConvertTo-SecureString -String "meridian" -Force -AsPlainText; '
        f'Export-PfxCertificate -Cert $cert -FilePath "{pfx_path}" -Password $pwd; '
        f'Write-Output "Cert created: $($cert.Thumbprint)"'
    )
    result = subprocess.run(
        ["powershell", "-NoProfile", "-Command", ps_cmd],
        capture_output=True, text=True, timeout=30,
        creationflags=0x08000000,
    )
    if result.returncode != 0:
        print(f"  WARNING: Could not create self-signed cert: {result.stderr.strip()}")
        print("  Falling back to unsigned build.")
        return pathlib.Path()

    print(f"  Self-signed cert created: {pfx_path}")
    return pfx_path


def _sign_binary(exe_path: pathlib.Path, pfx_path: pathlib.Path | None = None, pfx_pass: str | None = None) -> bool:
    """Sign the executable using PowerShell's Set-AuthenticodeSignature."""
    # Determine which PFX to use
    if pfx_path and pfx_path.exists():
        use_pfx = pfx_path
    elif os.environ.get("MERIDIAN_PFX_PATH"):
        use_pfx = pathlib.Path(os.environ["MERIDIAN_PFX_PATH"])
    else:
        use_pfx = _create_self_signed_cert()
        if not use_pfx.exists():
            return False
        pfx_pass = "meridian"

    if not use_pfx.exists():
        print(f"  WARNING: PFX not found: {use_pfx}")
        return False

    password = pfx_pass or os.environ.get("MERIDIAN_PFX_PASS", "meridian")

    print(f"  Signing {exe_path.name} with {use_pfx.name}...")
    # Use PowerShell Set-AuthenticodeSignature (available on all Windows)
    ps_cmd = (
        f'$pwd = ConvertTo-SecureString -String "{password}" -Force -AsPlainText; '
        f'$cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2("{use_pfx}", $pwd); '
        f'$sig = Set-AuthenticodeSignature -FilePath "{exe_path}" -Certificate $cert '
        f'-TimestampServer "http://timestamp.digicert.com" -HashAlgorithm SHA256; '
        f'Write-Output "Status: $($sig.Status)"; '
        f'if ($sig.StatusMessage) {{ Write-Output "Message: $($sig.StatusMessage)" }}'
    )
    result = subprocess.run(
        ["powershell", "-NoProfile", "-Command", ps_cmd],
        capture_output=True, text=True, timeout=120,
        creationflags=0x08000000,
    )
    if "Valid" in result.stdout:
        print("  Signed and trusted!")
        return True
    elif result.returncode == 0 and "Message" not in result.stdout:
        print("  Signed (self-signed cert - will show SmartScreen warning)")
        print("  For production, use a CA-signed certificate.")
        return True
    else:
        print(f"  WARNING: Signing failed: {result.stdout.strip()} {result.stderr.strip()}")
        return False


def build(sign: bool = False, pfx_path: str | None = None, pfx_pass: str | None = None):
    print(f"==> Building Meridian standalone executable from {SPEC}...")
    import tempfile
    import shutil

    workpath = pathlib.Path(tempfile.gettempdir()) / "pyi_meridian_work"
    distpath = pathlib.Path(tempfile.gettempdir()) / "pyi_meridian_dist"
    workpath.mkdir(parents=True, exist_ok=True)
    distpath.mkdir(parents=True, exist_ok=True)

    cmd = [
        sys.executable,
        "-m",
        "PyInstaller",
        "--clean",
        "--noconfirm",
        "--workpath", str(workpath),
        "--distpath", str(distpath),
        str(SPEC),
    ]
    subprocess.check_call(cmd, cwd=str(ROOT))

    target_name = "meridian.exe" if sys.platform == "win32" else "meridian"
    built_bin = distpath / target_name
    if not built_bin.exists():
        print(f"Build failed: binary not found in {distpath}")
        sys.exit(1)

    DIST.mkdir(parents=True, exist_ok=True)
    out_bin = DIST / target_name
    print(f"  Copying {built_bin} -> {out_bin}...")
    shutil.copy2(built_bin, out_bin)

    size_mb = out_bin.stat().st_size / (1024 * 1024)
    print(f"Build successful! Binary: {out_bin} ({size_mb:.1f} MB)")

    if sign and sys.platform == "win32":
        pfx = pathlib.Path(pfx_path) if pfx_path else None
        _sign_binary(out_bin, pfx, pfx_pass)

    print("\nDone!")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Build Meridian standalone binary")
    parser.add_argument("--sign", action="store_true", help="Sign the binary with a code signing certificate")
    parser.add_argument("--pfx", default=None, help="Path to .pfx certificate file")
    parser.add_argument("--pfx-pass", default=None, help="Password for the .pfx file")
    args = parser.parse_args()
    build(sign=args.sign, pfx_path=args.pfx, pfx_pass=args.pfx_pass)
