//! The Jev (TypeSafe System One) client.
//!
//! One call per voice turn: `POST {base_url}/systemone` with a `state` and a
//! set of typed questions; the response carries typed answers with
//! probabilities and confidence.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::JevSettings;
use crate::keys;
use crate::questions::{Answer, Questions};

#[derive(Debug, Error)]
pub enum JevError {
    #[error("no API key configured for Jev")]
    NoKey,
    #[error("jev request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("jev returned {status}: {body}")]
    Status { status: u16, body: String },
    #[error("could not decode jev response: {0}")]
    Decode(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct JevResponse {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub answers: BTreeMap<String, Answer>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

impl JevResponse {
    pub fn choice(&self, name: &str) -> Option<(&str, f64, &BTreeMap<String, f64>)> {
        match self.answers.get(name) {
            Some(Answer::Choice {
                choice,
                confidence,
                probabilities,
            }) => Some((choice.as_str(), *confidence, probabilities)),
            _ => None,
        }
    }

    pub fn noul(&self, name: &str) -> Option<f64> {
        match self.answers.get(name) {
            Some(Answer::Noul { noul }) => Some(*noul),
            _ => None,
        }
    }

    pub fn score(&self, name: &str) -> Option<(f64, f64)> {
        match self.answers.get(name) {
            Some(Answer::Score {
                score, confidence, ..
            }) => Some((*score, *confidence)),
            _ => None,
        }
    }
}

/// The routing backend. Implemented by [`HttpJev`] and the fixture-backed
/// [`crate::dummy::DummyJev`].
pub trait SystemOne {
    fn system_one(&self, state: &str, questions: &Questions) -> Result<JevResponse, JevError>;
}

#[derive(Debug, Serialize)]
struct Request<'a> {
    state: &'a str,
    model: &'a str,
    questions: &'a Questions,
}

#[derive(Debug)]
pub struct HttpJev {
    base_url: String,
    model: String,
    key: Option<String>,
    client: reqwest::blocking::Client,
}

impl HttpJev {
    /// Build a client, resolving the API key from the configured reference.
    /// The reference is resolved lazily per request so a key added to the
    /// keychain after startup is picked up.
    pub fn new(settings: &JevSettings) -> Result<Self, JevError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(settings.timeout_ms))
            .build()?;
        Ok(Self {
            base_url: settings.base_url.trim_end_matches('/').to_string(),
            model: settings.model.clone(),
            key: settings.key.clone(),
            client,
        })
    }
}

impl SystemOne for HttpJev {
    fn system_one(&self, state: &str, questions: &Questions) -> Result<JevResponse, JevError> {
        let secret = keys::resolve(self.key.as_deref()).ok_or(JevError::NoKey)?;
        let url = format!("{}/systemone", self.base_url);
        let body = Request {
            state,
            model: &self.model,
            questions,
        };
        let resp = self
            .client
            .post(&url)
            .bearer_auth(secret)
            .json(&body)
            .send()?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            return Err(JevError::Status {
                status: status.as_u16(),
                body: body.chars().take(400).collect(),
            });
        }
        let text = resp.text()?;
        Ok(serde_json::from_str(&text)?)
    }
}
