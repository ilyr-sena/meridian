"""CLI: detection, watching, info, orchestration.

Every subcommand resolves the current config from layered sources and then
delegates. Nothing here does network/pproc work on its own.
"""

from __future__ import annotations

import argparse
import logging
import sys
import traceback

from .config import Config
from .log import init as init_log

log = logging.getLogger(__name__)


def _verbosity_args() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(add_help=False)
    p.add_argument("-v", "--verbose", action="count", default=0, help="more logs")
    p.add_argument("-q", "--quiet", action="store_true", help="only errors")
    return p


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="meridian-py")

    # Global verbosity knobs are ignored in parsers constructed elsewhere;
    # they're consumed by cli.main.
    p.add_argument("-v", "--verbose", action="count", default=0)
    p.add_argument("-q", "--quiet", action="store_true")
    p.add_argument("--anisette-url", default=None, help="remote anisette server URL (e.g. http://vps:6969)")
    p.add_argument("--api-url", default=None, help="Meridian API URL for heartbeat/device sync (default: https://www.meridianhub.cc)")

    sub = p.add_subparsers(dest="cmd", required=False)

    sub.add_parser("list", help="show attached devices (enriched if possible)")

    p_info = sub.add_parser("info", help="full metadata for one device")
    p_info.add_argument("udid", nargs="?", default=None)

    sub.add_parser("watch", help="stream attach/detach events")

    p_run = sub.add_parser("run", help="full orchestrated session")
    p_run.add_argument("--udid", default=None)
    p_run.add_argument("--no-wda", action="store_true")
    p_run.add_argument("--no-hid", action="store_true")
    p_run.add_argument("--no-tunnels", action="store_true")
    p_run.add_argument(
        "--no-stream",
        action="store_true",
        help="skip launching the screen-stream probe on-device",
    )
    p_run.add_argument("--no-http", action="store_true", help="don't run the :9001 HTTP API")

    p_tun = sub.add_parser("tunnel", help="iproxy-style tunnels only")
    p_tun.add_argument("pairs", nargs="+");
    p_tun.add_argument("--udid", default=None)

    p_stream = sub.add_parser("stream", help="stream the screen and nothing else")
    p_stream.add_argument("--udid", default=None)
    p_stream.add_argument("--no-launch", action="store_true", help="skip launching the probe app on device")
    p_stream.add_argument("--open", action="store_true", help="open the player in a browser")

    p_sideload = sub.add_parser("sideload", help="sign and install an IPA onto the device over USB")
    p_sideload.add_argument("ipa", help="path to unsigned or signed IPA")
    p_sideload.add_argument("--udid", default=None, help="target device UDID")
    p_sideload.add_argument("--bundle-id", default=None, help="override bundle ID")
    p_sideload.add_argument("--apple-id", default=None, help="Apple ID email")
    p_sideload.add_argument("--password", default=None, help="Apple ID password")
    p_sideload.add_argument("--browser", action="store_true", help="use Chrome window instead of terminal login")
    p_sideload.add_argument("--renew", action="store_true", help="force profile renewal from Apple")
    p_sideload.add_argument("--login", action="store_true", help="re-authenticate with Apple")
    p_sideload.add_argument("--key", default=None, help="custom private key path (.pem/.p12)")
    p_sideload.add_argument("--cert", default=None, help="custom certificate path (.pem)")

    p_remote = sub.add_parser("remote", help="run the CoreDevice 60Hz HID touch & app bridge on :9001")
    p_remote.add_argument("--udid", default=None, help="target device UDID")
    p_remote.add_argument("--port", type=int, default=9001, help="bridge HTTP/WS port (default: 9001)")
    p_remote.add_argument("--host", default="127.0.0.1", help="bridge host (default: 127.0.0.1)")
    p_remote.add_argument("--mesh-key", default=None, help="Tailscale tsnet auth key for remote mesh access")

    p_hub = sub.add_parser("hub", help="launch the Meridian Desktop GUI hub or headless daemon")
    p_hub.add_argument("--cli", action="store_true", help="run in headless CLI daemon mode without GUI")
    p_hub.add_argument("--no-auto-start", action="store_true", help="don't automatically start streaming on ready devices")

    p_fw = sub.add_parser("firewall", help="manage Windows Firewall rules for Meridian")
    p_fw.add_argument("--remove", action="store_true", help="remove all Meridian firewall rules")

    return p


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    cfg = Config.from_sources(cli_args=args)
    cfg.ensure_dirs()
    init_log(_log_level(cfg))

    if cfg.anisette_url:
        from .core.vault import GLOBAL_VAULT
        GLOBAL_VAULT.set_anisette_url(cfg.anisette_url)
        log.info("anisette URL saved: %s", cfg.anisette_url)

    if cfg.api_url:
        from .core.vault import GLOBAL_VAULT
        GLOBAL_VAULT.set_api_url(cfg.api_url)
        log.info("API URL saved: %s", cfg.api_url)

    from .core.firewall import ensure_firewall_rules
    ensure_firewall_rules()

    try:
        match args.cmd:
            case "list":
                from .commands import list_devices_cmd
                return list_devices_cmd(cfg)
            case "info":
                from .commands import info_cmd
                return info_cmd(cfg, args.udid)
            case "watch":
                from .commands import watch_cmd
                return watch_cmd(cfg)
            case "run":
                from .runner import run_session
                return run_session(cfg, args)
            case "stream":
                from .commands import stream_cmd
                return stream_cmd(cfg, args)
            case "sideload":
                from .commands import sideload_cmd
                return sideload_cmd(cfg, args)
            case "tunnel":
                from .commands import tunnel_cmd
                return tunnel_cmd(cfg, args.pairs, args.udid)
            case "remote":
                from .commands import remote_cmd
                return remote_cmd(cfg, args)
            case "firewall":
                from .core.firewall import ensure_firewall_rules, remove_firewall_rules
                if getattr(args, "remove", False):
                    remove_firewall_rules()
                    print("Meridian firewall rules removed.")
                else:
                    ensure_firewall_rules()
                    print("Meridian firewall rules configured.")
                return 0
            case "hub" | None:
                from .ui.app import run_desktop_app
                cli_mode = getattr(args, "cli", False)
                auto_start = not getattr(args, "no_auto_start", False)
                return run_desktop_app(cli_mode=cli_mode, auto_start=auto_start, api_url=cfg.api_url)
            case other:
                parser.error(f"unknown command: {other}")
    except (SystemExit, KeyboardInterrupt):
        return 130
    except Exception:
        traceback.print_exc()
        return 2
    return 0


def _log_level(cfg: Config) -> int:
    import logging
    return {
        "trace": 5,
        "debug": logging.DEBUG,
        "info": logging.INFO,
        "error": logging.ERROR,
    }.get(cfg.log_level, logging.INFO)


if __name__ == "__main__":
    sys.exit(main())
