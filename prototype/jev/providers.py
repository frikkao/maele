"""Provider adapters. Each one turns a prompt into a completion string."""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from dataclasses import dataclass

from . import keys
from .config import Target


class ProviderError(RuntimeError):
    pass


@dataclass
class Completion:
    text: str
    target: str
    model: str


def _post_json(url: str, payload: dict, headers: dict, timeout: int = 60) -> dict:
    body = json.dumps(payload).encode()
    req = urllib.request.Request(url, data=body, method="POST")
    req.add_header("Content-Type", "application/json")
    for k, v in headers.items():
        req.add_header(k, v)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        detail = e.read().decode(errors="replace")[:400]
        raise ProviderError(f"{e.code} from {url}: {detail}") from e
    except urllib.error.URLError as e:
        raise ProviderError(f"could not reach {url}: {e.reason}") from e


def complete(target: Target, system: str, user: str) -> Completion:
    """Send a prompt to a target and return its text answer."""
    if not target.enabled:
        raise ProviderError(f"target '{target.name}' is disabled")

    if target.kind in ("openai_compat", "ollama"):
        base = target.base_url or (
            "http://localhost:11434/v1" if target.kind == "ollama" else ""
        )
        if not base:
            raise ProviderError(f"target '{target.name}' has no base_url")
        secret = keys.resolve(target.key)
        headers = {}
        if secret:
            headers["Authorization"] = f"Bearer {secret}"
        payload = {
            "model": target.model,
            "max_tokens": target.max_tokens,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        }
        data = _post_json(base.rstrip("/") + "/chat/completions", payload, headers)
        try:
            text = data["choices"][0]["message"]["content"]
        except (KeyError, IndexError, TypeError) as e:
            raise ProviderError(f"unexpected response shape from {target.name}") from e
        return Completion(text=text or "", target=target.name, model=target.model)

    if target.kind == "anthropic":
        secret = keys.resolve(target.key)
        if not secret:
            raise ProviderError(f"target '{target.name}' needs an API key")
        base = target.base_url or "https://api.anthropic.com"
        payload = {
            "model": target.model,
            "max_tokens": target.max_tokens,
            "system": system,
            "messages": [{"role": "user", "content": user}],
        }
        data = _post_json(
            base.rstrip("/") + "/v1/messages",
            payload,
            {"x-api-key": secret, "anthropic-version": "2023-06-01"},
        )
        parts = data.get("content") or []
        text = "".join(p.get("text", "") for p in parts if p.get("type") == "text")
        return Completion(text=text, target=target.name, model=target.model)

    raise ProviderError(f"target '{target.name}' has unknown kind '{target.kind}'")


def health(target: Target) -> tuple[bool, str]:
    """Cheap liveness check: is a key resolvable and a URL configured?"""
    if not target.enabled:
        return False, "disabled"
    if target.kind in ("openai_compat", "ollama") and not target.base_url:
        if target.kind != "ollama":
            return False, "no base_url"
    # anthropic defaults to https://api.anthropic.com when base_url is unset.
    if target.key and not keys.resolve(target.key):
        return False, f"missing secret ({target.key})"
    if not target.model and target.kind != "shell":
        return False, "no model set"
    return True, "ok"
