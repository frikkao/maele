//! Provider adapters. Each turns a prompt into a completion string.

use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::json;
use thiserror::Error;

use crate::config::Target;
use crate::keys;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("target '{0}' is disabled")]
    Disabled(String),
    #[error("target '{0}' has no base_url")]
    NoBaseUrl(String),
    #[error("target '{0}' needs an API key")]
    NoKey(String),
    #[error("target '{0}' has unknown kind '{1}'")]
    UnknownKind(String, String),
    #[error("request to {url} failed: {source}")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("{status} from {url}: {body}")]
    Status {
        status: u16,
        url: String,
        body: String,
    },
    #[error("unexpected response shape from '{0}'")]
    Shape(String),
}

fn client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("reqwest client")
}

fn post_json(
    url: &str,
    payload: serde_json::Value,
    headers: &[(&str, String)],
) -> Result<serde_json::Value, ProviderError> {
    let mut req = client().post(url).json(&payload);
    for (k, v) in headers {
        req = req.header(*k, v.as_str());
    }
    let resp = req.send().map_err(|source| ProviderError::Http {
        url: url.to_string(),
        source,
    })?;
    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    if !status.is_success() {
        return Err(ProviderError::Status {
            status: status.as_u16(),
            url: url.to_string(),
            body: body.chars().take(400).collect(),
        });
    }
    serde_json::from_str(&body).map_err(|_| ProviderError::Shape(url.to_string()))
}

/// Send a prompt to a target and return its text answer.
pub fn complete(target: &Target, system: &str, user: &str) -> Result<String, ProviderError> {
    if !target.enabled {
        return Err(ProviderError::Disabled(target.name.clone()));
    }

    match target.kind.as_str() {
        "openai_compat" | "ollama" => {
            let base = if !target.base_url.is_empty() {
                target.base_url.clone()
            } else if target.kind == "ollama" {
                "http://localhost:11434/v1".to_string()
            } else {
                return Err(ProviderError::NoBaseUrl(target.name.clone()));
            };
            let mut headers: Vec<(&str, String)> = Vec::new();
            if let Some(secret) = keys::resolve(target.key.as_deref()) {
                headers.push(("Authorization", format!("Bearer {secret}")));
            }
            let url = format!("{}/chat/completions", base.trim_end_matches('/'));
            let payload = json!({
                "model": target.model,
                "max_tokens": target.max_tokens,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
            });
            let data = post_json(&url, payload, &headers)?;
            data["choices"][0]["message"]["content"]
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| ProviderError::Shape(target.name.clone()))
        }
        "anthropic" => {
            let secret = keys::resolve(target.key.as_deref())
                .ok_or_else(|| ProviderError::NoKey(target.name.clone()))?;
            let base = if target.base_url.is_empty() {
                "https://api.anthropic.com".to_string()
            } else {
                target.base_url.clone()
            };
            let url = format!("{}/v1/messages", base.trim_end_matches('/'));
            let payload = json!({
                "model": target.model,
                "max_tokens": target.max_tokens,
                "system": system,
                "messages": [{"role": "user", "content": user}],
            });
            let headers = vec![
                ("x-api-key", secret),
                ("anthropic-version", "2023-06-01".to_string()),
            ];
            let data = post_json(&url, payload, &headers)?;
            let text = data["content"]
                .as_array()
                .map(|parts| {
                    parts
                        .iter()
                        .filter(|p| p["type"] == "text")
                        .filter_map(|p| p["text"].as_str())
                        .collect::<String>()
                })
                .unwrap_or_default();
            Ok(text)
        }
        other => Err(ProviderError::UnknownKind(
            target.name.clone(),
            other.to_string(),
        )),
    }
}

/// Cheap liveness check: is a key resolvable and a URL configured?
pub fn health(target: &Target) -> (bool, String) {
    if !target.enabled {
        return (false, "disabled".into());
    }
    if matches!(target.kind.as_str(), "openai_compat" | "ollama")
        && target.base_url.is_empty()
        && target.kind != "ollama"
    {
        return (false, "no base_url".into());
    }
    if let Some(ref r) = target.key {
        if keys::resolve(Some(r)).is_none() {
            return (false, format!("missing secret ({r})"));
        }
    }
    if target.model.is_empty() && target.kind != "shell" {
        return (false, "no model set".into());
    }
    (true, "ok".into())
}
