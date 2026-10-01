//! A fixture-backed Jev backend.
//!
//! Lets routing be exercised end to end with dummy API/data and no network.
//! A fixture file maps trigger tokens to a canned System One response; the
//! first matching rule wins, otherwise `default` is used.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::jev::{JevError, JevResponse, SystemOne};
use crate::questions::{Answer, Questions};

#[derive(Debug, Deserialize)]
struct DummyRule {
    #[serde(rename = "match", default)]
    matches: Vec<String>,
    response: JevResponse,
}

#[derive(Debug, Deserialize)]
struct DummyFile {
    #[serde(default)]
    responses: Vec<DummyRule>,
    #[serde(default)]
    default: Option<JevResponse>,
}

#[derive(Debug)]
pub struct DummyJev {
    rules: Vec<DummyRule>,
    default: Option<JevResponse>,
}

impl DummyJev {
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        let file: DummyFile = serde_json::from_str(text)?;
        Ok(Self {
            rules: file.responses,
            default: file.default,
        })
    }

    pub fn from_file(path: &Path) -> Result<Self, std::io::Error> {
        let text = std::fs::read_to_string(path)?;
        Self::from_json(&text).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

impl SystemOne for DummyJev {
    fn system_one(&self, state: &str, _questions: &Questions) -> Result<JevResponse, JevError> {
        let lower = state.to_lowercase();
        for rule in &self.rules {
            if rule
                .matches
                .iter()
                .any(|t| lower.contains(&t.to_lowercase()))
            {
                return Ok(rule.response.clone());
            }
        }
        if let Some(default) = &self.default {
            return Ok(default.clone());
        }
        // No fixtures matched: abstain so the cheap layers can decide.
        Ok(JevResponse {
            model: "dummy".into(),
            answers: BTreeMap::from([(
                "capability".to_string(),
                Answer::Choice {
                    choice: String::new(),
                    confidence: 0.0,
                    probabilities: BTreeMap::new(),
                },
            )]),
            usage: None,
        })
    }
}
