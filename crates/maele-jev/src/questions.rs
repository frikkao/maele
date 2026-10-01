//! The typed questions Maele asks Jev once per voice turn.
//!
//! State in, typed questions out: a `Choice` for the capability, a `Noul` for
//! urgency, and a `Score` for tone. Jev answers all three in one call and the
//! answers carry probabilities and confidence.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::Config;

/// A System One question. Serialises with a `type` tag, e.g.
/// `{"type":"choice","instructions":"...","criteria":{...}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    Choice {
        instructions: String,
        criteria: BTreeMap<String, String>,
    },
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
    Noul {
        instructions: String,
    },
}

/// A typed answer carrying the value plus probability and confidence.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Choice {
        choice: String,
        #[serde(default)]
        confidence: f64,
        #[serde(default)]
        probabilities: BTreeMap<String, f64>,
    },
    Score {
        score: f64,
        #[serde(default)]
        confidence: f64,
        #[serde(default)]
        legend: BTreeMap<String, String>,
        #[serde(default)]
        probabilities: BTreeMap<String, f64>,
    },
    Noul {
        noul: f64,
    },
}

pub type Questions = BTreeMap<String, Question>;

/// Build the standard Maele question set from the configured capabilities and
/// tone levels.
pub fn build(cfg: &Config) -> Questions {
    let mut q: Questions = BTreeMap::new();

    let mut capability = BTreeMap::new();
    for (name, cap) in &cfg.capabilities {
        let label = if cap.description.is_empty() {
            name.clone()
        } else {
            format!("{} — {}", name, cap.description)
        };
        capability.insert(name.clone(), label);
    }
    q.insert(
        "capability".into(),
        Question::Choice {
            instructions: "Which capability should handle this request?".into(),
            criteria: capability,
        },
    );

    q.insert(
        "urgency".into(),
        Question::Noul {
            instructions:
                "The request is time-sensitive, frustrated, or something is broken right now"
                    .into(),
        },
    );

    let levels = cfg.tone_levels();
    if !levels.is_empty() {
        q.insert(
            "tone".into(),
            Question::Score {
                instructions: "How much care should answering this request take?".into(),
                criteria: levels.into_iter().map(str::to_string).collect(),
            },
        );
    }

    q
}
