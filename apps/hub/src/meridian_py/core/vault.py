"""
Cross-platform encrypted credential vault for Meridian.

Stores sensitive credentials (Apple ID, Tailscale auth keys, MongoDB URI)
using host-bound AES-GCM/Fernet encryption:
  - Linux: ~/.local/share/meridian/vault.enc
  - Windows: %LOCALAPPDATA%\\Meridian\\vault.enc
"""
from __future__ import annotations

import base64
import hashlib
import json
import logging
import os
import pathlib
import sys
from typing import Any, Dict, Optional

from cryptography.fernet import Fernet
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.kdf.pbkdf2 import PBKDF2HMAC

log = logging.getLogger(__name__)


def get_meridian_data_dir() -> pathlib.Path:
    """Return standard cross-platform application data directory."""
    if sys.platform == "win32":
        appdata = os.environ.get("LOCALAPPDATA") or str(pathlib.Path.home() / "AppData" / "Local")
        p = pathlib.Path(appdata) / "Meridian"
    else:
        p = pathlib.Path.home() / ".local" / "share" / "meridian"
    p.mkdir(parents=True, exist_ok=True)
    return p


def get_meridian_cache_dir() -> pathlib.Path:
    """Return standard cross-platform cache directory."""
    if sys.platform == "win32":
        appdata = os.environ.get("LOCALAPPDATA") or str(pathlib.Path.home() / "AppData" / "Local")
        p = pathlib.Path(appdata) / "Meridian" / "Cache"
    else:
        p = pathlib.Path.home() / ".cache" / "meridian"
    p.mkdir(parents=True, exist_ok=True)
    return p


def _get_machine_fingerprint() -> bytes:
    """Generate a stable machine-bound salt for encrypting local vault credentials."""
    parts = [sys.platform, os.name]
    if sys.platform == "linux":
        for mid_path in ("/etc/machine-id", "/var/lib/dbus/machine-id"):
            if os.path.exists(mid_path):
                try:
                    parts.append(pathlib.Path(mid_path).read_text().strip())
                    break
                except OSError:
                    pass
    elif sys.platform == "win32":
        try:
            import winreg
            with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Cryptography") as k:
                guid, _ = winreg.QueryValueEx(k, "MachineGuid")
                parts.append(guid)
        except Exception:
            pass

    parts.append(pathlib.Path.home().name)
    raw = ":".join(parts).encode("utf-8")
    return hashlib.sha256(raw).digest()


class CredentialVault:
    """Encrypted key-value vault bound to the local machine."""

    def __init__(self, vault_path: Optional[pathlib.Path] = None):
        self.vault_path = vault_path or (get_meridian_data_dir() / "vault.enc")
        self._key = self._derive_key()
        self._fernet = Fernet(self._key)
        self._data: Dict[str, Any] = self._load()

    def _derive_key(self) -> bytes:
        salt = b"meridian-host-vault-salt-v1"
        kdf = PBKDF2HMAC(
            algorithm=hashes.SHA256(),
            length=32,
            salt=salt,
            iterations=100_000,
        )
        derived = kdf.derive(_get_machine_fingerprint())
        return base64.urlsafe_b64encode(derived)

    def _load(self) -> Dict[str, Any]:
        if not self.vault_path.exists():
            return {}
        try:
            encrypted = self.vault_path.read_bytes()
            decrypted = self._fernet.decrypt(encrypted)
            return json.loads(decrypted.decode("utf-8"))
        except Exception as e:
            log.warning("could not read encrypted vault (%s) — initializing clean", e)
            return {}

    def _save(self) -> None:
        try:
            raw = json.dumps(self._data).encode("utf-8")
            encrypted = self._fernet.encrypt(raw)
            # Atomic write
            tmp = self.vault_path.with_suffix(".tmp")
            tmp.write_bytes(encrypted)
            if sys.platform != "win32":
                tmp.chmod(0o600)
            tmp.replace(self.vault_path)
        except Exception as e:
            log.error("failed to save encrypted vault: %s", e)

    def get_secret(self, key: str, default: Optional[str] = None) -> Optional[str]:
        return self._data.get(key, default)

    def set_secret(self, key: str, value: str) -> None:
        self._data[key] = value
        self._save()

    def delete_secret(self, key: str) -> None:
        if key in self._data:
            del self._data[key]
            self._save()

    # Domain helpers
    def get_apple_credentials(self) -> tuple[Optional[str], Optional[str]]:
        return self.get_secret("apple_id"), self.get_secret("apple_password")

    def set_apple_credentials(self, apple_id: str, password: str) -> None:
        self.set_secret("apple_id", apple_id)
        self.set_secret("apple_password", password)

    def get_mesh_key(self) -> Optional[str]:
        return self.get_secret("mesh_key")

    def set_mesh_key(self, key: str) -> None:
        self.set_secret("mesh_key", key)

    def get_mongodb_uri(self) -> Optional[str]:
        return self.get_secret("mongodb_uri")

    def set_mongodb_uri(self, uri: str) -> None:
        self.set_secret("mongodb_uri", uri)

    def get_anisette_url(self) -> Optional[str]:
        return self.get_secret("anisette_url")

    def set_anisette_url(self, url: str) -> None:
        self.set_secret("anisette_url", url)

    def get_api_url(self) -> Optional[str]:
        return self.get_secret("api_url")

    def set_api_url(self, url: str) -> None:
        self.set_secret("api_url", url)


# Global singleton vault
GLOBAL_VAULT = CredentialVault()
