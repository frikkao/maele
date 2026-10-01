"""Config loading and validation.

The config is the whole product surface: it declares which targets exist, which
capabilities they serve, and the rules that bind a question to a capability.
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path

import yaml

DEFAULT_PATHS = [
    Path.cwd() / "jev.yaml",
    Path.home() / ".config" / "jev" / "config.yaml",
]


class ConfigError(RuntimeError):
    pass


@dataclass
class Target:
    """Where a question can be sent."""

    name: str
    kind: str = "openai_compat"  # openai_compat | anthropic | ollama | shell | shortcut
    model: str = ""
    base_url: str = ""
    key: str | None = None  # keychain:<service> | env:<VAR>
    command: list[str] = field(default_factory=list)  # for kind=shell
    url_scheme: str = ""  # for kind=shortcut
    max_tokens: int = 1024
    enabled: bool = True


@dataclass
class Capability:
    name: str
    target: str
    description: str = ""
    examples: list[str] = field(default_factory=list)


@dataclass
class Rule:
    capability: str
    when_any: list[str] = field(default_factory=list)
    when_all: list[str] = field(default_factory=list)
    none_of: list[str] = field(default_factory=list)
    priority: int = 50
    languages: list[str] = field(default_factory=list)  # e.g. ["nb", "en"]


@dataclass
class Config:
    targets: dict[str, Target]
    capabilities: dict[str, Capability]
    rules: list[Rule]
    fallback: str = ""
    confidence_floor: float = 0.35
    layers: list[str] = field(default_factory=lambda: ["rules", "semantic"])
    llm_router: Target | None = None
    source: Path | None = None

    # -- lookup ---------------------------------------------------------

    def target_for(self, capability: str) -> Target | None:
        cap = self.capabilities.get(capability)
        if not cap:
            return None
        return self.targets.get(cap.target)

    def resolve_target(self, name: str) -> Target | None:
        return self.targets.get(name)

    def default_capability(self) -> str:
        return self.fallback


def load(path: str | Path | None = None) -> Config:
    """Load and validate a Jev config."""
    if path is None:
        for candidate in DEFAULT_PATHS:
            if candidate.is_file():
                path = candidate
                break
        else:
            raise ConfigError(
                "no config found; looked in "
                + ", ".join(str(p) for p in DEFAULT_PATHS)
                + " — copy config.example.yaml to ~/.config/jev/config.yaml"
            )
    path = Path(path).expanduser()
    if not path.is_file():
        raise ConfigError(f"config not found: {path}")

    raw = yaml.safe_load(path.read_text()) or {}

    targets = {}
    for name, spec in (raw.get("targets") or {}).items():
        spec = spec or {}
        targets[name] = Target(
            name=name,
            kind=spec.get("kind", "openai_compat"),
            model=spec.get("model", ""),
            base_url=spec.get("base_url", ""),
            key=spec.get("key"),
            command=list(spec.get("command") or []),
            url_scheme=spec.get("url_scheme", ""),
            max_tokens=int(spec.get("max_tokens", 1024)),
            enabled=bool(spec.get("enabled", True)),
        )
    if not targets:
        raise ConfigError("config has no targets")

    capabilities = {}
    for name, spec in (raw.get("capabilities") or {}).items():
        spec = spec or {}
        target = spec.get("target")
        if target not in targets:
            raise ConfigError(f"capability '{name}' points at unknown target '{target}'")
        capabilities[name] = Capability(
            name=name,
            target=target,
            description=spec.get("description", ""),
            examples=list(spec.get("examples") or []),
        )
    if not capabilities:
        raise ConfigError("config has no capabilities")

    rules = []
    for spec in raw.get("rules") or []:
        cap = spec.get("capability")
        if cap not in capabilities:
            raise ConfigError(f"rule points at unknown capability '{cap}'")
        rules.append(
            Rule(
                capability=cap,
                when_any=[s.lower() for s in spec.get("when_any") or []],
                when_all=[s.lower() for s in spec.get("when_all") or []],
                none_of=[s.lower() for s in spec.get("none_of") or []],
                priority=int(spec.get("priority", 50)),
                languages=list(spec.get("languages") or []),
            )
        )

    fallback = raw.get("fallback") or next(iter(capabilities))
    if fallback not in capabilities:
        raise ConfigError(f"fallback '{fallback}' is not a capability")

    routing = raw.get("routing") or {}
    llm_spec = routing.get("llm_router") or {}
    llm_router = None
    if llm_spec:
        llm_router = Target(
            name="__llm_router__",
            kind=llm_spec.get("kind", "openai_compat"),
            model=llm_spec.get("model", ""),
            base_url=llm_spec.get("base_url", ""),
            key=llm_spec.get("key"),
            max_tokens=int(llm_spec.get("max_tokens", 16)),
        )

    return Config(
        targets=targets,
        capabilities=capabilities,
        rules=rules,
        fallback=fallback,
        confidence_floor=float(routing.get("confidence_floor", 0.35)),
        layers=list(routing.get("layers") or ["rules", "semantic"]),
        llm_router=llm_router,
        source=path,
    )


def example_config() -> str:
    return (Path(__file__).parent.parent / "config.example.yaml").read_text()
