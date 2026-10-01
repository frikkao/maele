"""The Jev decision layer.

Layers run cheapest-first and stop as soon as one is confident enough:

    rules  (microseconds)  → semantic (~ms) → llm (100s of ms) → fallback

Routing to the wrong target is worse than routing slowly, so anything below the
configured confidence floor falls through to the fallback capability rather than
guessing.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from . import matcher
from .config import Config, Rule, Target


@dataclass
class Candidate:
    capability: str
    score: float
    layer: str
    reason: str = ""


@dataclass
class Decision:
    capability: str
    target: Target | None
    confidence: float
    layer: str
    reason: str
    language: str = "und"
    candidates: list[Candidate] = field(default_factory=list)

    def as_dict(self) -> dict:
        return {
            "capability": self.capability,
            "target": self.target.name if self.target else None,
            "confidence": round(self.confidence, 3),
            "layer": self.layer,
            "reason": self.reason,
            "language": self.language,
            "candidates": [
                {"capability": c.capability, "score": round(c.score, 3), "layer": c.layer}
                for c in self.candidates
            ],
        }


# -- layer 1: rules -------------------------------------------------------


def score_rule(text: str, rule: Rule, language: str) -> tuple[float, str]:
    """Return (strength 0..1, human reason) for one rule against one utterance."""
    if rule.languages and language not in rule.languages:
        return 0.0, "language filter"
    if rule.none_of and any(p in text for p in rule.none_of):
        return 0.0, "excluded by none_of"
    if rule.when_all and not all(p in text for p in rule.when_all):
        return 0.0, "missing a required term"

    hits = [p for p in rule.when_any if p in text]
    if rule.when_any and not hits:
        return 0.0, "no trigger term"
    if not rule.when_any and not rule.when_all:
        return 0.0, "rule has no conditions"

    # Every trigger is declared deliberately, so a single match is already a
    # confident signal. Scaling by hits/total would mean a rule with 19 triggers
    # scored one keyword as 5% strength — which is how a keyword layer silently
    # becomes a semantic layer. Reward additional hits instead.
    strength = min(1.0, 0.60 + 0.15 * (len(hits) - 1))
    reason = "matched " + ", ".join(hits) if hits else "matched required terms"
    return strength, reason


def rules_layer(text: str, cfg: Config, language: str) -> list[Candidate]:
    out = []
    for rule in cfg.rules:
        strength, reason = score_rule(text, rule, language)
        if strength <= 0:
            continue
        # Priority breaks ties; it must not dominate the match itself.
        score = min(1.0, strength * 0.9 + rule.priority / 1000.0)
        out.append(Candidate(rule.capability, score, "rules", reason))
    return out


# -- layer 2: semantic ----------------------------------------------------


def semantic_layer(text: str, sim: matcher.Similarity) -> list[Candidate]:
    out = []
    for capability, score in sim.scores(text).items():
        if score <= 0:
            continue
        out.append(Candidate(capability, score, "semantic", f"similarity {score:.2f}"))
    return out


# -- layer 3: llm ---------------------------------------------------------

_ROUTER_SYSTEM = (
    "You are a routing classifier. You never answer the question. "
    "Reply with exactly one capability name from the list and nothing else."
)


def llm_layer(text: str, cfg: Config, language: str) -> list[Candidate]:
    from . import providers

    if cfg.llm_router is None:
        return []
    menu = "\n".join(
        f"- {name}: {cap.description or '(no description)'}"
        for name, cap in cfg.capabilities.items()
    )
    user = (
        f"Capabilities:\n{menu}\n\n"
        f"Detected language: {language}\n"
        f"Question: {text}\n\n"
        "Which capability should handle this? Reply with the name only."
    )
    try:
        comp = providers.complete(cfg.llm_router, _ROUTER_SYSTEM, user)
    except providers.ProviderError:
        return []
    answer = comp.text.strip().lower().strip(".`\"'")
    for name in cfg.capabilities:
        if name.lower() in answer:
            return [Candidate(name, 0.75, "llm", f"model chose {name}")]
    return []


# -- the router -----------------------------------------------------------


class Jev:
    def __init__(self, cfg: Config) -> None:
        self.cfg = cfg
        examples = {
            name: cap.examples
            for name, cap in cfg.capabilities.items()
            if cap.examples
        }
        self.similarity = matcher.Similarity().fit(examples)

    def decide(self, text: str) -> Decision:
        text_l = " ".join(text.lower().split())
        language = matcher.detect_language(text)
        candidates: list[Candidate] = []
        chosen: Candidate | None = None
        layer_used = "fallback"

        for layer in self.cfg.layers:
            if layer == "rules":
                found = rules_layer(text_l, self.cfg, language)
            elif layer == "semantic":
                found = semantic_layer(text, self.similarity)
            elif layer == "llm":
                found = llm_layer(text, self.cfg, language)
            else:
                found = []
            candidates.extend(found)
            if found:
                best = max(found, key=lambda c: c.score)
                if best.score >= self.cfg.confidence_floor:
                    chosen = best
                    layer_used = layer
                    break

        candidates.sort(key=lambda c: c.score, reverse=True)

        if chosen is None:
            capability = self.cfg.fallback
            return Decision(
                capability=capability,
                target=self.cfg.target_for(capability),
                confidence=0.0,
                layer="fallback",
                reason="no layer cleared the confidence floor",
                language=language,
                candidates=candidates,
            )

        return Decision(
            capability=chosen.capability,
            target=self.cfg.target_for(chosen.capability),
            confidence=chosen.score,
            layer=layer_used,
            reason=chosen.reason,
            language=language,
            candidates=candidates,
        )

    def answer(self, text: str) -> tuple[Decision, str]:
        """Route, then ask the chosen target. Returns (decision, answer)."""
        from . import providers

        decision = self.decide(text)
        if decision.target is None:
            return decision, f"[jev] no target resolved for '{decision.capability}'"
        system = (
            "You are a concise voice assistant. Answer in the language the "
            "question was asked in. Keep it short: this is read aloud."
        )
        comp = providers.complete(decision.target, system, text)
        return decision, comp.text
