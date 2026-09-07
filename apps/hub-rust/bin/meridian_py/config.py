"""Layered configuration: built-in defaults ← config file (TOML) ← CLI flags.

No global magic: everything flows through `Config.load()` which is explicitly
called by cli.py with parsed args.
"""

import os
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Optional

try:
    import tomllib  # type: ignore
except ImportError:  # python < 3.11
    tomllib = None  # type: ignore


def _state_dir() -> Path:
    if sys.platform == "win32":
        appdata = os.environ.get("LOCALAPPDATA") or str(Path.home() / "AppData" / "Local")
        return Path(appdata) / "Meridian" / "state"
    return Path.home() / ".local" / "state" / "meridian"


def _config_dir() -> Path:
    if sys.platform == "win32":
        appdata = os.environ.get("APPDATA") or str(Path.home() / "AppData" / "Roaming")
        return Path(appdata) / "Meridian" / "config"
    return Path.home() / ".config" / "meridian"


STATE_DIR = _state_dir()
CONFIG_DIR = _config_dir()
DEFAULT_CONFIG_PATH = CONFIG_DIR / "config.toml"


@dataclass
class Config:
    # Daemon / endpoint
    mux_endpoint: str = ""   # empty = auto-detect; unix socket path or tcp://host:port
    iproxy_ports: list[tuple[int, int]] = field(
        default_factory=lambda: [(9200, 9200), (8100, 8100)]
    )
    tunneld_port: int = 49151
    http_port: int = 9001

    # State & persistence
    state_dir: Path = STATE_DIR

    # Behaviors
    skip_wda: bool = False
    no_probe: bool = False
    udid: Optional[str] = None        # optional explicit device
    log_level: str = "info"

    # Sideload / Anisette
    anisette_url: str = ""   # remote anisette server URL (e.g. http://vps:6969); empty = start local

    # API / MongoDB
    api_url: str = "https://www.meridianhub.cc"   # central Meridian API for heartbeat/device sync

    # Bundles
    wda_bundle: str = "dev.ius.meridian.runner.xctrunner.SRTHYBYH35"
    probe_bundle: str = "dev.ius.meridian.runner.xctrunner.SRTHYBYH35"

    @classmethod
    def defaults(cls) -> "Config":
        return cls()

    @classmethod
    def from_sources(
        cls,
        cli_args: Optional[Any],
        config_path: Optional[Path] = None,
    ) -> "Config":
        cfg = cls.defaults()

        # Layer 1: TOML file
        cfg_path = config_path if config_path is not None else DEFAULT_CONFIG_PATH
        if cfg_path.exists():
            if tomllib is None:
                raise RuntimeError("Python 3.11+ required for TOML config")
            with open(cfg_path, "rb") as f:
                table = tomllib.load(f)
            cfg.merge_table(table)

        # Layer 2: CLI flags
        if cli_args is not None:
            cfg.merge_cli(cli_args)

        return cfg

    def merge_table(self, t: dict[str, Any]) -> None:
        if "mux_endpoint" in t: self.mux_endpoint = str(t["mux_endpoint"])
        if "tunneld_port" in t: self.tunneld_port = int(t["tunneld_port"])
        if "http_port" in t: self.http_port = int(t["http_port"])
        if "skip_wda" in t: self.skip_wda = bool(t["skip_wda"])
        if "udid" in t: self.udid = str(t["udid"])
        if "anisette_url" in t: self.anisette_url = str(t["anisette_url"])
        if "api_url" in t: self.api_url = str(t["api_url"])
        if "wda_bundle" in t: self.wda_bundle = str(t["wda_bundle"])
        if "probe_bundle" in t: self.probe_bundle = str(t["probe_bundle"])
        if "iproxy_ports" in t:
            # Expect [[local, remote], ...]
            self.iproxy_ports = [tuple(p) for p in t["iproxy_ports"]]

    def merge_cli(self, args: Any) -> None:
        def pick(name: str):
            v = getattr(args, name, None)
            if v is not None:
                setattr(self, name, v)
        pick("udid")
        pick("anisette_url")
        pick("api_url")
        if getattr(args, "skip_wda", False):  self.skip_wda = True
        if getattr(args, "no_probe", False): self.no_probe = True
        if getattr(args, "verbose", 0) >= 2:  self.log_level = "trace"
        elif getattr(args, "verbose", 0) >= 1: self.log_level = "debug"
        if getattr(args, "quiet", False):      self.log_level = "error"

    def ensure_dirs(self) -> None:
        self.state_dir.mkdir(parents=True, exist_ok=True)
        CONFIG_DIR.mkdir(parents=True, exist_ok=True)
