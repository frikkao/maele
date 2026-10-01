"""Command line entry point: inspect routing, manage keys, run a query."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from . import config as config_mod
from . import keys, providers
from .router import Jev

DEFAULT_CONFIG = Path.home() / ".config" / "jev" / "config.yaml"


def _load(args) -> config_mod.Config:
    return config_mod.load(args.config)


def _fmt(decision) -> str:
    tgt = decision.target
    tgt_s = f"{tgt.name} ({tgt.kind}/{tgt.model})" if tgt else "none"
    lines = [
        f"  capability : {decision.capability}",
        f"  target     : {tgt_s}",
        f"  confidence : {decision.confidence:.2f}   via {decision.layer}",
        f"  language   : {decision.language}",
        f"  reason     : {decision.reason}",
    ]
    if decision.candidates:
        lines.append("  candidates :")
        for c in decision.candidates[:4]:
            lines.append(f"      {c.score:.2f}  {c.capability}  [{c.layer}]")
    return "\n".join(lines)


def cmd_route(args) -> int:
    cfg = _load(args)
    jev = Jev(cfg)
    text = " ".join(args.text)
    decision = jev.decide(text)
    if args.json:
        print(json.dumps(decision.as_dict(), indent=2, ensure_ascii=False))
    else:
        print(f"jev: {text!r}")
        print(_fmt(decision))
    return 0


def cmd_ask(args) -> int:
    cfg = _load(args)
    jev = Jev(cfg)
    text = " ".join(args.text)
    decision, answer = jev.answer(text)
    if not args.quiet:
        print(f"jev → {decision.capability} "
              f"[{decision.layer} {decision.confidence:.2f}, {decision.language}]")
    print(answer)
    return 0


def cmd_doctor(args) -> int:
    cfg = _load(args)
    print(f"config: {cfg.source}")
    print(f"layers: {', '.join(cfg.layers)}   fallback: {cfg.fallback}   "
          f"floor: {cfg.confidence_floor}")
    print("\ntargets:")
    worst = 0
    for name, tgt in cfg.targets.items():
        ok, note = providers.health(tgt)
        worst = max(worst, 0 if ok else 1)
        mark = "ok " if ok else "!! "
        print(f"  {mark}{name:<16} {tgt.kind:<14} {tgt.model or '-':<28} {note}")
    print("\ncapabilities:")
    for name, cap in cfg.capabilities.items():
        tgt = cfg.targets.get(cap.target)
        ok = "ok" if tgt and providers.health(tgt)[0] else "no target"
        print(f"  {name:<12} → {cap.target:<16} {len(cap.examples)} examples   [{ok}]")
    return worst


def cmd_keys(args) -> int:
    service = args.service
    if args.keys_action == "set":
        import getpass

        secret = getpass.getpass(f"secret for {service}: ")
        if not secret:
            print("aborted: empty secret", file=sys.stderr)
            return 1
        keys.keychain_set(service, secret)
        print(f"stored {service} in the login keychain")
        return 0
    if args.keys_action == "check":
        found = keys._keychain_get(service)
        print(f"{service}: {'present' if found else 'NOT SET'}")
        return 0 if found else 1
    if args.keys_action == "delete":
        print("deleted" if keys.keychain_delete(service) else "nothing to delete")
        return 0
    if args.keys_action == "init":
        dest = args.config or DEFAULT_CONFIG
        dest.parent.mkdir(parents=True, exist_ok=True)
        if dest.exists() and not args.force:
            print(f"{dest} already exists (use --force)", file=sys.stderr)
            return 1
        dest.write_text(config_mod.example_config())
        print(f"wrote {dest}")
        return 0
    return 1


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="jev", description="route a question to the right model")
    p.add_argument("--config", help="path to config.yaml")
    sub = p.add_subparsers(dest="cmd", required=True)

    r = sub.add_parser("route", help="show where a question would go")
    r.add_argument("text", nargs="+")
    r.add_argument("--json", action="store_true")
    r.set_defaults(func=cmd_route)

    a = sub.add_parser("ask", help="route the question and answer it")
    a.add_argument("text", nargs="+")
    a.add_argument("--quiet", action="store_true", help="print only the answer")
    a.set_defaults(func=cmd_ask)

    d = sub.add_parser("doctor", help="check config and target health")
    d.set_defaults(func=cmd_doctor)

    k = sub.add_parser("keys", help="manage API keys in the macOS keychain")
    k.add_argument("keys_action", choices=["set", "check", "delete", "init"])
    k.add_argument("service", nargs="?", default="")
    k.add_argument("--force", action="store_true", help="init: overwrite existing config")
    k.set_defaults(func=cmd_keys)

    args = p.parse_args(argv)
    try:
        return args.func(args)
    except config_mod.ConfigError as e:
        print(f"config error: {e}", file=sys.stderr)
        return 2
    except providers.ProviderError as e:
        print(f"provider error: {e}", file=sys.stderr)
        return 3


if __name__ == "__main__":
    raise SystemExit(main())
