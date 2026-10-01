//! Routing tests. Run with: cargo test -p maele-jev

use std::collections::HashMap;
use std::path::Path;

use maele_jev::config;
use maele_jev::dummy::DummyJev;
use maele_jev::matcher;
use maele_jev::policy::{Decision, Router};

const CFG_YAML: &str = r#"
routing:
  layers: [jev, rules, semantic]
  confidence_floor: 0.35
fallback: general
tone:
  quick:   { model_tier: small }
  serious: { model_tier: large }
overrides:
  opus: smart
targets:
  cheap: { kind: openai_compat, base_url: "http://x/v1", model: "m", key: "env:NOPE" }
  smart: { kind: anthropic, model: "a", key: "env:NOPE" }
  local: { kind: ollama, base_url: "http://localhost:11434/v1", model: "l" }
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
  writing:
    target: cheap
    examples: ["skriv en e-post", "summarise this in three points"]
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
"#;

fn cfg() -> config::Config {
    config::parse(CFG_YAML, Path::new("test.yaml")).expect("test config parses")
}

fn router() -> Router {
    Router::new(cfg())
}

fn fixture_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/dummy_jev.json")
}

fn dummy_router() -> Router {
    let backend = DummyJev::from_file(&fixture_path()).expect("dummy fixtures load");
    Router::with_backend(cfg(), Some(Box::new(backend)))
}

// -- cheap layers ------------------------------------------------------------

#[test]
fn rules_win_on_keyword() {
    let d = router().decide("logg dette i idebanken");
    assert_eq!(d.capability, "mine");
    assert_eq!(d.layer, "rules");
    assert!(d.confidence >= 0.35);
}

#[test]
fn rule_language_filter_blocks() {
    let d = router().decide("docker containeren min dør hele tiden");
    assert!(
        d.layer != "rules" || d.capability != "code",
        "Norwegian phrasing must not trip the English-only code rule"
    );
    assert!(matches!(
        d.capability.as_str(),
        "general" | "code" | "research"
    ));
}

#[test]
fn rule_language_filter_allows_english() {
    let d = router().decide("why is my docker container exiting");
    assert_eq!(d.capability, "code");
    assert_eq!(d.layer, "rules");
}

#[test]
fn priority_breaks_ties_not_dominates() {
    let d = router().decide("pull request with a segfault");
    assert_eq!(d.capability, "code");
}

#[test]
fn semantic_fallback_when_no_keyword() {
    let d = router().decide("hva er hovedstaden i australia egentlig");
    assert_eq!(d.capability, "general");
    assert!(matches!(d.layer.as_str(), "semantic" | "rules"));
}

#[test]
fn unknown_question_falls_back() {
    let d = router().decide("zzzz qqqq xyzzy");
    assert_eq!(d.layer, "fallback");
    assert_eq!(d.capability, "general");
    assert_eq!(d.confidence, 0.0);
}

#[test]
fn language_detection() {
    assert_eq!(
        matcher::detect_language("hvorfor feiler bygget på skolen"),
        "nb"
    );
    assert_eq!(matcher::detect_language("why is the build failing"), "en");
    assert_eq!(matcher::detect_language("blåbærsyltetøy"), "nb");
    assert_eq!(matcher::detect_language(""), "und");
}

#[test]
fn config_rejects_unknown_target() {
    let bad = r#"
targets:
  a: { kind: ollama, base_url: "http://x/v1", model: "m" }
capabilities:
  general: { target: does_not_exist }
"#;
    let err = config::parse(bad, Path::new("bad.yaml")).unwrap_err();
    assert!(err.to_string().contains("unknown target"), "{err}");
}

#[test]
fn similarity_scores_are_bounded() {
    let c = cfg();
    let examples: HashMap<String, Vec<String>> = c
        .capabilities
        .values()
        .filter(|c| !c.examples.is_empty())
        .map(|c| (c.name.clone(), c.examples.clone()))
        .collect();
    let sim = matcher::Similarity::new().fit(&examples);
    let scores = sim.scores("hva er de siste nyhetene om renten i dag");
    assert!(!scores.is_empty(), "expected at least one score");
    assert!(scores.values().all(|v| (0.0..=1.0).contains(v)));
}

// -- Jev (dummy backend) -----------------------------------------------------

#[test]
fn jev_routes_code_as_serious() {
    let d = dummy_router().decide("why is my docker container exiting with 137");
    assert_eq!(d.capability, "code");
    assert_eq!(d.layer, "jev");
    assert_eq!(d.tier.as_deref(), Some("large"));
    assert!(d.confidence >= 0.35);
    assert_eq!(d.target.as_ref().map(|t| t.name.as_str()), Some("smart"));
}

#[test]
fn jev_routes_calendar_as_quick() {
    let d = dummy_router().decide("hva har jeg på kalenderen i morgen");
    assert_eq!(d.capability, "mine");
    assert_eq!(d.layer, "jev");
    assert_eq!(d.tier.as_deref(), Some("small"));
}

#[test]
fn jev_reports_probabilities() {
    let d = dummy_router().decide("compare the iphone and pixel cameras");
    assert_eq!(d.capability, "research");
    assert!(d.probabilities.contains_key("research"));
}

#[test]
fn jev_abstains_and_falls_through_to_rules() {
    // The dummy fixture matches "zzzz" first and abstains (confidence 0), so
    // routing must fall through to the English code rule.
    let d = dummy_router().decide("why docker zzzz");
    assert_eq!(d.layer, "rules");
    assert_eq!(d.capability, "code");
}

#[test]
fn override_wins_over_jev() {
    let d = dummy_router().decide("use opus for this one");
    assert_eq!(d.layer, "override");
    assert_eq!(d.overridden.as_deref(), Some("opus"));
    assert_eq!(d.target.as_ref().map(|t| t.name.as_str()), Some("smart"));
}

#[test]
fn dummy_without_fixture_abstains() {
    let backend = DummyJev::from_json(r#"{"responses":[],"default":null}"#).unwrap();
    let d = Router::with_backend(cfg(), Some(Box::new(backend))).decide("zzzz qqqq xyzzy");
    assert_eq!(d.layer, "fallback");
}

#[test]
fn decision_serialises() {
    let d: Decision = dummy_router().decide("why is my docker container exiting");
    let json = d.as_json();
    assert_eq!(json["capability"], "code");
    assert_eq!(json["layer"], "jev");
    assert_eq!(json["target"], "smart");
    assert!(json["confidence"].is_number());
}
