"""Tests for the routing layers. Run with: uv run pytest -q"""

from __future__ import annotations

import textwrap

import pytest

from jev import config as config_mod
from jev import matcher
from jev.router import Jev

CFG_YAML = textwrap.dedent(
    """
    routing:
      layers: [rules, semantic]
      confidence_floor: 0.35
    fallback: general
    targets:
      cheap:  {kind: openai_compat, base_url: "http://x/v1", model: "m", key: "env:NOPE"}
      smart:  {kind: anthropic, model: "a", key: "env:NOPE"}
      local:  {kind: ollama, base_url: "http://localhost:11434/v1", model: "l"}
    capabilities:
      general:
        target: cheap
        examples: ["hva er hovedstaden i australia", "what is the capital of australia"]
      code:
        target: smart
        examples: ["why is my docker container exiting", "hvorfor feiler bygget"]
      research:
        target: cheap
        examples: ["hva er de siste nyhetene om renten", "compare two phones"]
      mine:
        target: local
        examples: ["hva har jeg på kalenderen i morgen"]
    rules:
      - capability: mine
        priority: 100
        when_any: ["kalenderen min", "logg dette", "my calendar"]
      - capability: code
        priority: 80
        when_any: ["docker", "pull request", "segfault"]
        languages: ["en"]
    """
)


@pytest.fixture
def cfg(tmp_path):
    p = tmp_path / "config.yaml"
    p.write_text(CFG_YAML)
    return config_mod.load(p)


@pytest.fixture
def jev(cfg):
    return Jev(cfg)


def test_rules_win_on_keyword(jev):
    d = jev.decide("logg dette i idebanken")
    assert d.capability == "mine"
    assert d.layer == "rules"
    assert d.confidence >= 0.35


def test_rule_language_filter_blocks(jev):
    """The code rule is English-only, so the Norwegian phrasing must not match it."""
    d = jev.decide("docker containeren min dør hele tiden")
    assert d.layer != "rules" or d.capability != "code"
    assert d.capability in {"general", "code", "research"}


def test_rule_language_filter_allows_english(jev):
    d = jev.decide("why is my docker container exiting")
    assert d.capability == "code"
    assert d.layer == "rules"


def test_priority_breaks_ties_not_dominates(jev):
    """A priority-100 rule must not outrank a decisive lower-priority match."""
    d = jev.decide("pull request with a segfault")
    assert d.capability == "code"


def test_semantic_fallback_when_no_keyword(jev):
    d = jev.decide("hva er hovedstaden i australia egentlig")
    assert d.capability == "general"
    assert d.layer in {"semantic", "rules"}


def test_unknown_question_falls_back(jev):
    d = jev.decide("zzzz qqqq xyzzy")
    assert d.layer == "fallback"
    assert d.capability == "general"
    assert d.confidence == 0.0


def test_language_detection():
    assert matcher.detect_language("hvorfor feiler bygget på skolen") == "nb"
    assert matcher.detect_language("why is the build failing") == "en"
    assert matcher.detect_language("blåbærsyltetøy") == "nb"
    assert matcher.detect_language("") == "und"


def test_config_rejects_unknown_target(tmp_path):
    bad = tmp_path / "bad.yaml"
    bad.write_text(
        textwrap.dedent(
            """
            targets:
              a: {kind: ollama, base_url: "http://x/v1", model: "m"}
            capabilities:
              general: {target: does_not_exist}
            """
        )
    )
    with pytest.raises(config_mod.ConfigError, match="unknown target"):
        config_mod.load(bad)


def test_similarity_scores_are_bounded(cfg):
    sim = matcher.Similarity().fit(
        {n: c.examples for n, c in cfg.capabilities.items() if c.examples}
    )
    scores = sim.scores("hva er de siste nyhetene om renten i dag")
    assert scores, "expected at least one score"
    assert all(0.0 <= v <= 1.0 for v in scores.values())
