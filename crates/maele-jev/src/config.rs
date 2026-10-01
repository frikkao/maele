//! Config loading and validation.
//!
//! The config is the whole product surface: it declares which targets exist,
//! which capabilities they serve, the rules that bind a question to a
//! capability, and how Jev is reached.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Deserialize;
use thiserror::Error;

pub const EXAMPLE_CONFIG: &str = include_str!("../../../config.example.yaml");

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("no config found; looked in {looked} — copy config.example.yaml to ~/.config/maele/config.yaml")]
    NotFound { looked: String },
    #[error("config not found: {0}")]
    Missing(PathBuf),
    #[error("config error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("config has no targets")]
    NoTargets,
    #[error("config has no capabilities")]
    NoCapabilities,
    #[error("capability '{cap}' points at unknown target '{target}'")]
    UnknownTarget { cap: String, target: String },
    #[error("rule points at unknown capability '{0}'")]
    UnknownRuleCapability(String),
    #[error("fallback '{0}' is not a capability")]
    BadFallback(String),
    #[error("could not read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub fn default_paths() -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from("maele.yaml"), PathBuf::from(".maele.yaml")];
    if let Some(home) = directories::BaseDirs::new() {
        paths.push(home.home_dir().join(".config/maele/config.yaml"));
    }
    paths
}

/// Where a question can be sent.
#[derive(Debug, Clone)]
pub struct Target {
    pub name: String,
    pub kind: String,
    pub model: String,
    pub base_url: String,
    pub key: Option<String>,
    pub command: Vec<String>,
    pub url_scheme: String,
    pub max_tokens: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct Capability {
    pub name: String,
    pub target: String,
    pub description: String,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub capability: String,
    pub when_any: Vec<String>,
    pub when_all: Vec<String>,
    pub none_of: Vec<String>,
    pub priority: i32,
    pub languages: Vec<String>,
}

/// How to reach the Jev (System One) endpoint.
#[derive(Debug, Clone)]
pub struct JevSettings {
    pub base_url: String,
    pub model: String,
    pub key: Option<String>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ToneTier {
    pub model_tier: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub targets: BTreeMap<String, Target>,
    pub capabilities: BTreeMap<String, Capability>,
    pub rules: Vec<Rule>,
    pub fallback: String,
    pub confidence_floor: f64,
    pub layers: Vec<String>,
    pub jev: Option<JevSettings>,
    pub tone: IndexMap<String, ToneTier>,
    pub overrides: BTreeMap<String, String>,
    pub source: PathBuf,
}

impl Config {
    pub fn target_for(&self, capability: &str) -> Option<&Target> {
        let cap = self.capabilities.get(capability)?;
        self.targets.get(&cap.target)
    }

    pub fn resolve_target(&self, name: &str) -> Option<&Target> {
        self.targets.get(name)
    }

    /// Resolve an override value to a target: the value may be a target or a
    /// capability name (which then resolves to its target).
    pub fn resolve_override(&self, value: &str) -> Option<&Target> {
        self.targets.get(value).or_else(|| self.target_for(value))
    }

    pub fn tone_levels(&self) -> Vec<&str> {
        self.tone.keys().map(String::as_str).collect()
    }
}

// -- raw (deserialize) shapes ------------------------------------------------

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct TargetSpec {
    kind: String,
    model: String,
    base_url: String,
    key: Option<String>,
    command: Vec<String>,
    url_scheme: String,
    max_tokens: u32,
    enabled: Option<bool>,
}

impl TargetSpec {
    fn into_target(self, name: &str) -> Target {
        Target {
            name: name.to_string(),
            kind: if self.kind.is_empty() {
                "openai_compat".into()
            } else {
                self.kind
            },
            model: self.model,
            base_url: self.base_url,
            key: self.key.filter(|k| !k.is_empty()),
            command: self.command,
            url_scheme: self.url_scheme,
            max_tokens: if self.max_tokens == 0 {
                1024
            } else {
                self.max_tokens
            },
            enabled: self.enabled.unwrap_or(true),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct CapabilitySpec {
    target: String,
    description: String,
    examples: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RuleSpec {
    capability: String,
    when_any: Vec<String>,
    when_all: Vec<String>,
    none_of: Vec<String>,
    priority: i32,
    languages: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct JevSpec {
    base_url: String,
    model: String,
    key: Option<String>,
    timeout_ms: u64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RoutingSpec {
    layers: Vec<String>,
    confidence_floor: f64,
    jev: Option<JevSpec>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawConfig {
    targets: BTreeMap<String, TargetSpec>,
    capabilities: BTreeMap<String, CapabilitySpec>,
    rules: Vec<RuleSpec>,
    fallback: Option<String>,
    routing: RoutingSpec,
    tone: IndexMap<String, ToneRaw>,
    overrides: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ToneRaw {
    model_tier: String,
}

impl From<ToneRaw> for ToneTier {
    fn from(t: ToneRaw) -> Self {
        ToneTier {
            model_tier: t.model_tier,
        }
    }
}

// -- loading -----------------------------------------------------------------

pub fn load(path: Option<&Path>) -> Result<Config, ConfigError> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => default_paths()
            .into_iter()
            .find(|p| p.is_file())
            .ok_or_else(|| ConfigError::NotFound {
                looked: default_paths()
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
            })?,
    };
    if !path.is_file() {
        return Err(ConfigError::Missing(path));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| ConfigError::Io {
        path: path.clone(),
        source: e,
    })?;
    parse(&text, &path)
}

pub fn parse(text: &str, source: &Path) -> Result<Config, ConfigError> {
    let raw: RawConfig = serde_yaml::from_str(text)?;

    let mut targets = BTreeMap::new();
    for (name, spec) in raw.targets {
        targets.insert(name.clone(), spec.into_target(&name));
    }
    if targets.is_empty() {
        return Err(ConfigError::NoTargets);
    }

    let mut capabilities = BTreeMap::new();
    for (name, spec) in raw.capabilities {
        if !targets.contains_key(&spec.target) {
            return Err(ConfigError::UnknownTarget {
                cap: name,
                target: spec.target,
            });
        }
        capabilities.insert(
            name.clone(),
            Capability {
                name,
                target: spec.target,
                description: spec.description,
                examples: spec.examples,
            },
        );
    }
    if capabilities.is_empty() {
        return Err(ConfigError::NoCapabilities);
    }

    let mut rules = Vec::new();
    for spec in raw.rules {
        if !capabilities.contains_key(&spec.capability) {
            return Err(ConfigError::UnknownRuleCapability(spec.capability));
        }
        rules.push(Rule {
            capability: spec.capability,
            when_any: spec
                .when_any
                .into_iter()
                .map(|s| s.to_lowercase())
                .collect(),
            when_all: spec
                .when_all
                .into_iter()
                .map(|s| s.to_lowercase())
                .collect(),
            none_of: spec.none_of.into_iter().map(|s| s.to_lowercase()).collect(),
            priority: if spec.priority == 0 {
                50
            } else {
                spec.priority
            },
            languages: spec.languages,
        });
    }

    let fallback = raw
        .fallback
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| capabilities.keys().next().cloned().unwrap_or_default());
    if !capabilities.contains_key(&fallback) {
        return Err(ConfigError::BadFallback(fallback));
    }

    let layers = if raw.routing.layers.is_empty() {
        vec!["jev".to_string(), "rules".into(), "semantic".into()]
    } else {
        raw.routing.layers
    };

    let jev = raw.routing.jev.map(|j| JevSettings {
        base_url: if j.base_url.is_empty() {
            "https://api.typesafe.ai/v1".into()
        } else {
            j.base_url
        },
        model: if j.model.is_empty() {
            "jev-latest".into()
        } else {
            j.model
        },
        key: j.key.filter(|k| !k.is_empty()),
        timeout_ms: if j.timeout_ms == 0 {
            1500
        } else {
            j.timeout_ms
        },
    });

    let tone: IndexMap<String, ToneTier> =
        raw.tone.into_iter().map(|(k, v)| (k, v.into())).collect();

    Ok(Config {
        targets,
        capabilities,
        rules,
        fallback,
        confidence_floor: if raw.routing.confidence_floor == 0.0 {
            0.35
        } else {
            raw.routing.confidence_floor
        },
        layers,
        jev,
        tone,
        overrides: raw.overrides,
        source: source.to_path_buf(),
    })
}
