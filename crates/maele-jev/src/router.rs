//! The cheap layers: declared rules and character-trigram similarity.
//!
//! These run as Jev's fail-open path — when Jev is unavailable, abstains, or
//! falls below the confidence floor.

use crate::config::{Config, Rule};
use crate::matcher;

#[derive(Debug, Clone)]
pub struct Candidate {
    pub capability: String,
    pub score: f64,
    pub layer: String,
    pub reason: String,
}

/// Return `(strength 0..1, human reason)` for one rule against one utterance.
pub fn score_rule(text: &str, rule: &Rule, language: &str) -> (f64, String) {
    if !rule.languages.is_empty() && !rule.languages.iter().any(|l| l == language) {
        return (0.0, "language filter".into());
    }
    if rule.none_of.iter().any(|p| text.contains(p.as_str())) {
        return (0.0, "excluded by none_of".into());
    }
    if !rule.when_all.iter().all(|p| text.contains(p.as_str())) {
        return (0.0, "missing a required term".into());
    }

    let hits: Vec<&str> = rule
        .when_any
        .iter()
        .filter(|p| text.contains(p.as_str()))
        .map(String::as_str)
        .collect();
    if !rule.when_any.is_empty() && hits.is_empty() {
        return (0.0, "no trigger term".into());
    }
    if rule.when_any.is_empty() && rule.when_all.is_empty() {
        return (0.0, "rule has no conditions".into());
    }

    // Every trigger is declared deliberately, so a single match is already a
    // confident signal. Reward additional hits rather than scaling by fraction.
    let strength = (0.60 + 0.15 * (hits.len().saturating_sub(1) as f64)).min(1.0);
    let reason = if hits.is_empty() {
        "matched required terms".to_string()
    } else {
        format!("matched {}", hits.join(", "))
    };
    (strength, reason)
}

pub fn rules_layer(text_lower: &str, cfg: &Config, language: &str) -> Vec<Candidate> {
    let mut out = Vec::new();
    for rule in &cfg.rules {
        let (strength, reason) = score_rule(text_lower, rule, language);
        if strength <= 0.0 {
            continue;
        }
        // Priority breaks ties; it must not dominate the match itself.
        let score = (strength * 0.9 + rule.priority as f64 / 1000.0).min(1.0);
        out.push(Candidate {
            capability: rule.capability.clone(),
            score,
            layer: "rules".into(),
            reason,
        });
    }
    out
}

pub fn semantic_layer(text: &str, sim: &matcher::Similarity) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (capability, score) in sim.scores(text) {
        if score <= 0.0 {
            continue;
        }
        out.push(Candidate {
            capability,
            score,
            layer: "semantic".into(),
            reason: format!("similarity {score:.2}"),
        });
    }
    out
}
