//! The routing policy: one Jev call per turn, cheap fail-open layers behind it.
//!
//! Order of play (configurable via `routing.layers`):
//!
//! 1. an explicit override in the transcript ("use opus") — always wins;
//! 2. `jev` — the brain; if it errors, is unconfigured, abstains, or falls
//!    below the confidence floor, routing falls through rather than guessing;
//! 3. `rules`, then `semantic` — the cheap fail-open path;
//! 4. `fallback` — never a guess.

use std::collections::BTreeMap;

use crate::config::{Config, Target};
use crate::jev::{HttpJev, JevError, JevResponse, SystemOne};
use crate::matcher::{self, Similarity};
use crate::questions;
use crate::router::{self, Candidate};

#[derive(Debug, Clone)]
pub struct Decision {
    pub capability: String,
    pub target: Option<Target>,
    pub tier: Option<String>,
    pub confidence: f64,
    pub layer: String,
    pub reason: String,
    pub language: String,
    pub probabilities: BTreeMap<String, f64>,
    pub overridden: Option<String>,
    pub candidates: Vec<Candidate>,
    pub jev_error: Option<String>,
}

impl Decision {
    pub fn as_json(&self) -> serde_json::Value {
        let cands: Vec<serde_json::Value> = self
            .candidates
            .iter()
            .map(|c| {
                serde_json::json!({
                    "capability": c.capability,
                    "score": round3(c.score),
                    "layer": c.layer,
                })
            })
            .collect();
        serde_json::json!({
            "capability": self.capability,
            "target": self.target.as_ref().map(|t| t.name.clone()),
            "target_kind": self.target.as_ref().map(|t| t.kind.clone()),
            "model": self.target.as_ref().map(|t| t.model.clone()),
            "tier": self.tier,
            "confidence": round3(self.confidence),
            "layer": self.layer,
            "reason": self.reason,
            "language": self.language,
            "probabilities": self.probabilities,
            "overridden": self.overridden,
            "jev_error": self.jev_error,
            "candidates": cands,
        })
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// A Jev layer result: the candidate, the resolved model tier, and the
/// capability probabilities.
type JevOutcome = (Candidate, Option<String>, BTreeMap<String, f64>);

pub struct Router {
    cfg: Config,
    sim: Similarity,
    backend: Option<Box<dyn SystemOne>>,
}

impl Router {
    /// Build a router, wiring the configured Jev endpoint when present.
    pub fn new(cfg: Config) -> Self {
        let backend = cfg
            .jev
            .as_ref()
            .and_then(|s| HttpJev::new(s).ok())
            .map(|c| Box::new(c) as Box<dyn SystemOne>);
        Self::with_backend(cfg, backend)
    }

    /// Build a router with an explicit backend (e.g. the fixture `DummyJev`).
    pub fn with_backend(cfg: Config, backend: Option<Box<dyn SystemOne>>) -> Self {
        let examples: std::collections::HashMap<String, Vec<String>> = cfg
            .capabilities
            .iter()
            .filter(|(_, c)| !c.examples.is_empty())
            .map(|(n, c)| (n.clone(), c.examples.clone()))
            .collect();
        let sim = Similarity::new().fit(&examples);
        Self { cfg, sim, backend }
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn decide(&self, text: &str) -> Decision {
        let text_l = normalize(text);
        let language = matcher::detect_language(text).to_string();
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut jev_error: Option<String> = None;

        if let Some((token, target)) = self.detect_override(&text_l) {
            let capability = self
                .cfg
                .capabilities
                .values()
                .find(|c| c.target == target.name)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| target.name.clone());
            return Decision {
                capability,
                target: Some(target),
                tier: None,
                confidence: 1.0,
                layer: "override".into(),
                reason: format!("explicit override '{token}'"),
                language,
                probabilities: BTreeMap::new(),
                overridden: Some(token),
                candidates,
                jev_error: None,
            };
        }

        for layer in self.cfg.layers.clone() {
            match layer.as_str() {
                "jev" => {
                    let Some(backend) = &self.backend else {
                        continue;
                    };
                    match self.jev_decide(backend.as_ref(), text) {
                        Ok((cand, tier, probs)) => {
                            if cand.score >= self.cfg.confidence_floor
                                && !cand.capability.is_empty()
                            {
                                return self.finish(cand, tier, probs, language, candidates, None);
                            }
                            candidates.push(cand);
                        }
                        Err(e) => jev_error = Some(e.to_string()),
                    }
                }
                "rules" => {
                    let found = router::rules_layer(&text_l, &self.cfg, &language);
                    if let Some(best) = pick(&found, self.cfg.confidence_floor) {
                        let best = best.clone();
                        candidates.extend(found);
                        return self.finish(
                            best,
                            None,
                            BTreeMap::new(),
                            language,
                            candidates,
                            jev_error.clone(),
                        );
                    }
                    candidates.extend(found);
                }
                "semantic" => {
                    let found = router::semantic_layer(text, &self.sim);
                    if let Some(best) = pick(&found, self.cfg.confidence_floor) {
                        let best = best.clone();
                        candidates.extend(found);
                        return self.finish(
                            best,
                            None,
                            BTreeMap::new(),
                            language,
                            candidates,
                            jev_error.clone(),
                        );
                    }
                    candidates.extend(found);
                }
                _ => {}
            }
        }

        candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
        let capability = self.cfg.fallback.clone();
        Decision {
            target: self.cfg.target_for(&capability).cloned(),
            capability,
            tier: None,
            confidence: 0.0,
            layer: "fallback".into(),
            reason: "no layer cleared the confidence floor".into(),
            language,
            probabilities: BTreeMap::new(),
            overridden: None,
            candidates,
            jev_error,
        }
    }

    fn finish(
        &self,
        cand: Candidate,
        tier: Option<String>,
        probabilities: BTreeMap<String, f64>,
        language: String,
        mut candidates: Vec<Candidate>,
        jev_error: Option<String>,
    ) -> Decision {
        candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
        let target = self.cfg.target_for(&cand.capability).cloned();
        let reason = cand.reason.clone();
        Decision {
            capability: cand.capability,
            target,
            tier,
            confidence: cand.score,
            layer: cand.layer,
            reason,
            language,
            probabilities,
            overridden: None,
            candidates,
            jev_error,
        }
    }

    fn jev_decide(&self, backend: &dyn SystemOne, text: &str) -> Result<JevOutcome, JevError> {
        let qs = questions::build(&self.cfg);
        let resp = backend.system_one(text, &qs)?;

        let (capability, confidence, probabilities, reason) = match resp.choice("capability") {
            Some((choice, confidence, probs)) if !choice.is_empty() => (
                choice.to_string(),
                confidence,
                probs.clone(),
                format!("jev chose {choice} ({confidence:.2})"),
            ),
            _ => (
                String::new(),
                0.0,
                BTreeMap::new(),
                "jev abstained".to_string(),
            ),
        };

        let candidate = Candidate {
            capability,
            score: confidence,
            layer: "jev".into(),
            reason,
        };
        Ok((candidate, self.derive_tier(&resp), probabilities))
    }

    fn derive_tier(&self, resp: &JevResponse) -> Option<String> {
        let levels = self.cfg.tone_levels();
        if levels.is_empty() {
            return None;
        }
        let idx = if let Some((score, _)) = resp.score("tone") {
            score.round().max(0.0) as usize
        } else {
            let noul = resp.noul("urgency")?;
            if noul >= 0.5 {
                levels.len() - 1
            } else {
                0
            }
        };
        let idx = idx.min(levels.len() - 1);
        let name = levels[idx];
        self.cfg.tone.get(name).map(|t| t.model_tier.clone())
    }

    fn detect_override(&self, text_l: &str) -> Option<(String, Target)> {
        let words: Vec<&str> = text_l.split_whitespace().collect();
        const FILLER: &[&str] = &[
            "the", "a", "an", "et", "en", "den", "det", "model", "modellen",
        ];
        for (i, w) in words.iter().enumerate() {
            if matches!(*w, "use" | "bruk" | "bruke") {
                for next in words.iter().skip(i + 1).take(2) {
                    let token = next.trim_matches(|c: char| !c.is_alphanumeric());
                    if token.is_empty() || FILLER.contains(&token) {
                        continue;
                    }
                    if let Some(val) = self.cfg.overrides.get(token) {
                        if let Some(t) = self.cfg.resolve_override(val) {
                            return Some((token.to_string(), t.clone()));
                        }
                    }
                    if let Some(t) = self.cfg.resolve_override(token) {
                        return Some((token.to_string(), t.clone()));
                    }
                }
            }
        }
        None
    }
}

fn pick(found: &[Candidate], floor: f64) -> Option<&Candidate> {
    found
        .iter()
        .filter(|c| c.score >= floor)
        .max_by(|a, b| a.score.total_cmp(&b.score))
}

fn normalize(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
