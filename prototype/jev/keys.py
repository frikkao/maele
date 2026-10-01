"""API key storage.

Keys are referenced in config as ``keychain:<service>`` or ``env:<VAR>`` and
resolved at call time. Secrets never live in the config file.
"""

from __future__ import annotations

import os
import subprocess


class KeyError_(RuntimeError):
    pass


def _keychain_get(service: str, account: str = "jev") -> str | None:
    """Read a generic password from the macOS login keychain."""
    try:
        out = subprocess.run(
            ["security", "find-generic-password", "-s", service, "-a", account, "-w"],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if out.returncode != 0:
        return None
    return out.stdout.strip() or None


def keychain_set(service: str, secret: str, account: str = "jev") -> None:
    """Store or replace a secret. Overwrites an existing entry."""
    # -U updates in place when the item already exists.
    res = subprocess.run(
        [
            "security", "add-generic-password",
            "-s", service, "-a", account,
            "-w", secret, "-U",
        ],
        capture_output=True,
        text=True,
        timeout=10,
    )
    if res.returncode != 0:
        raise KeyError_(f"could not write {service} to keychain: {res.stderr.strip()}")


def keychain_delete(service: str, account: str = "jev") -> bool:
    res = subprocess.run(
        ["security", "delete-generic-password", "-s", service, "-a", account],
        capture_output=True,
        text=True,
        timeout=10,
    )
    return res.returncode == 0


def resolve(ref: str | None) -> str | None:
    """Resolve a key reference to its secret.

    Accepts ``keychain:<service>``, ``env:<VAR>``, or a bare env var name.
    Returns None when the reference is empty or the secret is absent.
    """
    if not ref:
        return None
    if ref.startswith("keychain:"):
        return _keychain_get(ref.split(":", 1)[1])
    if ref.startswith("env:"):
        return os.environ.get(ref.split(":", 1)[1]) or None
    return os.environ.get(ref) or None
