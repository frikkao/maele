//! API key storage.
//!
//! Keys are referenced in config as `keychain:<service>` or `env:<VAR>` and
//! resolved at call time. Secrets never live in the config file.

use std::process::Command;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum KeyError {
    #[error("could not write {service} to keychain: {detail}")]
    Keychain { service: String, detail: String },
}

/// Read a generic password from the macOS login keychain.
pub fn keychain_get(service: &str) -> Option<String> {
    keychain_get_account(service, "maele")
}

pub fn keychain_get_account(service: &str, account: &str) -> Option<String> {
    let out = Command::new("security")
        .args(["find-generic-password", "-s", service, "-a", account, "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Store or replace a secret. Overwrites an existing entry (`-U`).
pub fn keychain_set(service: &str, secret: &str) -> Result<(), KeyError> {
    keychain_set_account(service, "maele", secret)
}

pub fn keychain_set_account(service: &str, account: &str, secret: &str) -> Result<(), KeyError> {
    let out = Command::new("security")
        .args([
            "add-generic-password",
            "-s",
            service,
            "-a",
            account,
            "-w",
            secret,
            "-U",
        ])
        .output()
        .map_err(|e| KeyError::Keychain {
            service: service.to_string(),
            detail: e.to_string(),
        })?;
    if !out.status.success() {
        return Err(KeyError::Keychain {
            service: service.to_string(),
            detail: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }
    Ok(())
}

pub fn keychain_delete(service: &str) -> bool {
    Command::new("security")
        .args(["delete-generic-password", "-s", service, "-a", "maele"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Resolve a key reference to its secret.
///
/// Accepts `keychain:<service>`, `env:<VAR>`, or a bare env var name.
/// Returns `None` when the reference is empty or the secret is absent.
pub fn resolve(reference: Option<&str>) -> Option<String> {
    let reference = reference?;
    if reference.is_empty() {
        return None;
    }
    if let Some(service) = reference.strip_prefix("keychain:") {
        return keychain_get(service);
    }
    if let Some(var) = reference.strip_prefix("env:") {
        return non_empty(std::env::var(var).ok());
    }
    non_empty(std::env::var(reference).ok())
}

fn non_empty(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.is_empty())
}
