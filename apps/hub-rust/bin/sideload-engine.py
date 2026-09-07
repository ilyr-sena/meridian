#!/usr/bin/env python3
"""Standalone Sideloading Execution Engine for Meridian Hub.
Signs IPA using Apple Developer Services and installs over USB via usbmuxd.
"""
import sys
import os
import argparse
from pathlib import Path

# 1. Dependency check & automatic re-execution with virtualenv if missing
try:
    import srp
    import pymobiledevice3
except ImportError:
    home = Path.home()
    venv_pythons = [
        home / "provision-venv" / "bin" / "python3",
        home / "provision-venv" / "bin" / "python",
        home / "provision-venv" / "Scripts" / "python.exe",
        Path("apps/hub/.venv/bin/python"),
        Path("apps/hub/.venv/Scripts/python.exe"),
        Path(".venv/bin/python"),
        Path(".venv/Scripts/python.exe"),
    ]
    for vp in venv_pythons:
        if vp.is_file() and os.access(vp, os.X_OK):
            if os.environ.get("_MERIDIAN_REEXEC") != "1":
                os.environ["_MERIDIAN_REEXEC"] = "1"
                os.execv(str(vp), [str(vp)] + sys.argv)

# 2. Add search paths for meridian_py
current = Path(__file__).resolve().parent
candidates = [
    current,                          # e.g. dist/bin (where meridian_py was copied)
    current.parent,                   # e.g. dist
    current / "meridian_py",
]

# Walk up up to 6 parent directories to locate apps/hub/src
walker = current
for _ in range(6):
    candidates.append(walker / "apps" / "hub" / "src")
    candidates.append(walker / "src")
    walker = walker.parent

for c in candidates:
    if (c / "meridian_py").is_dir():
        if str(c) not in sys.path:
            sys.path.insert(0, str(c))
        break

try:
    from meridian_py.sideload import sideload_app
except ImportError as err:
    print(f"[ERROR] Failed to load sideload engine module: {err}", flush=True)
    sys.exit(1)

def main():
    parser = argparse.ArgumentParser(description="Meridian Sideload Engine")
    parser.add_argument("--ipa", required=True, help="Path to unsigned IPA")
    parser.add_argument("--udid", required=True, help="Device UDID")
    parser.add_argument("--apple-id", required=True, help="Apple ID email")
    parser.add_argument("--password", required=True, help="Apple ID password")
    parser.add_argument("--anisette", default="http://100.51.75.20:6969", help="Anisette server URL")
    args = parser.parse_args()

    print("[PROGRESS:10] Connecting to Apple authentication service...", flush=True)

    try:
        print("[PROGRESS:25] Provisioning development certificate...", flush=True)
        target_bid = sideload_app(
            ipa_path=args.ipa,
            udid=args.udid,
            apple_id=args.apple_id,
            password=args.password,
            anisette_url=args.anisette,
            force_renew=False,
            force_login=False,
        )
        print(f"[PROGRESS:100] Sideload complete! Bundle: {target_bid}", flush=True)
        return 0
    except Exception as e:
        import traceback
        err_msg = str(e)
        print(f"[ERROR] {err_msg}", flush=True)
        traceback.print_exc(file=sys.stderr)
        return 1

if __name__ == "__main__":
    sys.exit(main())

