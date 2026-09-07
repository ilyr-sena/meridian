#!/usr/bin/env python3
"""Standalone Sideloading Execution Engine for Meridian Hub.
Signs IPA using Apple Developer Services and installs over USB via usbmuxd.
"""
import sys
import argparse
import json
from pathlib import Path

# Add python source paths
ROOT = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(ROOT / "apps" / "hub" / "src"))

def main():
    parser = argparse.ArgumentParser(description="Meridian Sideload Engine")
    parser.add_argument("--ipa", required=True, help="Path to unsigned IPA")
    parser.add_argument("--udid", required=True, help="Device UDID")
    parser.add_argument("--apple-id", required=True, help="Apple ID email")
    parser.add_argument("--password", required=True, help="Apple ID password")
    parser.add_argument("--anisette", default="http://100.51.75.20:6969", help="Anisette server URL")
    args = parser.parse_args()

    print("[PROGRESS:10] Connecting to Apple authentication service...", flush=True)

    from meridian_py.sideload import sideload_app

    def on_progress(pct, msg):
        print(f"[PROGRESS:{pct}] {msg}", flush=True)

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
