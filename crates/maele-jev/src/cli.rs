//! Command line entry point: inspect routing, replay decisions, manage keys.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use crate::config::{self, Config};
use crate::{
    dummy::DummyJev,
    keys, log,
    policy::{Decision, Router},
    providers,
};

const DEFAULT_DUMMY: &str = "fixtures/dummy_jev.json";

#[derive(Parser, Debug)]
#[command(name = "maele", about = "route a spoken question to the right model")]
pub struct Cli {
    /// Path to config.yaml
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Show where a question would go
    Route {
        #[arg(required = true)]
        text: Vec<String>,
        #[arg(long)]
        json: bool,
        /// Use the fixture-backed Jev backend instead of the network
        #[arg(long)]
        dummy: bool,
        #[arg(long, default_value = DEFAULT_DUMMY)]
        dummy_file: PathBuf,
        /// Do not append to the decision log
        #[arg(long)]
        no_log: bool,
    },
    /// Show the factors behind the last routing decision
    Explain,
    /// Route the question and answer it
    Ask {
        #[arg(required = true)]
        text: Vec<String>,
        #[arg(long)]
        quiet: bool,
        #[arg(long)]
        dummy: bool,
        #[arg(long, default_value = DEFAULT_DUMMY)]
        dummy_file: PathBuf,
        #[arg(long)]
        no_log: bool,
    },
    /// Check config and target health
    Doctor,
    /// Manage API keys in the macOS keychain
    Keys {
        #[command(subcommand)]
        action: KeysAction,
    },
    /// Install or inspect the config file
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum KeysAction {
    Set {
        service: String,
    },
    Check {
        service: String,
    },
    Delete {
        service: String,
    },
    Init {
        #[arg(long)]
        force: bool,
        path: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Write an example config
    Init {
        #[arg(long)]
        force: bool,
        path: Option<PathBuf>,
    },
}

pub fn run() -> i32 {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Route {
            text,
            json,
            dummy,
            dummy_file,
            no_log,
        } => cmd_route(
            cli.config.as_deref(),
            &text,
            json,
            dummy,
            &dummy_file,
            no_log,
        ),
        Command::Explain => cmd_explain(),
        Command::Ask {
            text,
            quiet,
            dummy,
            dummy_file,
            no_log,
        } => cmd_ask(
            cli.config.as_deref(),
            &text,
            quiet,
            dummy,
            &dummy_file,
            no_log,
        ),
        Command::Doctor => cmd_doctor(cli.config.as_deref()),
        Command::Keys { action } => cmd_keys(action, cli.config.as_deref()),
        Command::Config { action } => cmd_config(action, cli.config.as_deref()),
    }
}

fn load_config(path: Option<&Path>) -> Result<Config, i32> {
    config::load(path).map_err(|e| {
        eprintln!("config error: {e}");
        2
    })
}

fn build_router(path: Option<&Path>, dummy: bool, dummy_file: &Path) -> Result<Router, i32> {
    let cfg = load_config(path)?;
    if dummy {
        let backend = DummyJev::from_file(dummy_file).map_err(|e| {
            eprintln!("dummy fixtures error ({}): {e}", dummy_file.display());
            1
        })?;
        Ok(Router::with_backend(cfg, Some(Box::new(backend))))
    } else {
        Ok(Router::new(cfg))
    }
}

fn fmt(decision: &Decision) -> String {
    let tgt = match &decision.target {
        Some(t) => format!("{} ({}/{})", t.name, t.kind, t.model),
        None => "none".to_string(),
    };
    let mut lines = vec![
        format!("  capability : {}", decision.capability),
        format!("  target     : {tgt}"),
        format!(
            "  confidence : {:.2}   via {}",
            decision.confidence, decision.layer
        ),
        format!("  tier       : {}", decision.tier.as_deref().unwrap_or("-")),
        format!("  language   : {}", decision.language),
        format!("  reason     : {}", decision.reason),
    ];
    if let Some(err) = &decision.jev_error {
        lines.push(format!("  jev error  : {err}"));
    }
    if !decision.candidates.is_empty() {
        lines.push("  candidates :".to_string());
        for c in decision.candidates.iter().take(4) {
            lines.push(format!(
                "      {:.2}  {}  [{}]",
                c.score, c.capability, c.layer
            ));
        }
    }
    lines.join("\n")
}

fn cmd_route(
    config_path: Option<&Path>,
    text: &[String],
    json: bool,
    dummy: bool,
    dummy_file: &Path,
    no_log: bool,
) -> i32 {
    let router = match build_router(config_path, dummy, dummy_file) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let text = text.join(" ");
    let decision = router.decide(&text);
    if !no_log {
        let _ = log::append(&text, &decision);
    }
    if json {
        let mut v = decision.as_json();
        v["text"] = serde_json::Value::String(text);
        println!("{}", serde_json::to_string_pretty(&v).unwrap());
    } else {
        println!("maele: {text:?}");
        println!("{}", fmt(&decision));
    }
    0
}

fn cmd_ask(
    config_path: Option<&Path>,
    text: &[String],
    quiet: bool,
    dummy: bool,
    dummy_file: &Path,
    no_log: bool,
) -> i32 {
    let router = match build_router(config_path, dummy, dummy_file) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let text = text.join(" ");
    let decision = router.decide(&text);
    if !no_log {
        let _ = log::append(&text, &decision);
    }
    if !quiet {
        eprintln!(
            "maele → {} [{}, {:.2}, {}]",
            decision.capability, decision.layer, decision.confidence, decision.language
        );
    }
    let Some(target) = &decision.target else {
        println!("[maele] no target resolved for '{}'", decision.capability);
        return 0;
    };
    let system = "You are a concise voice assistant. Answer in the language the \
        question was asked in. Keep it short: this is read aloud.";
    match providers::complete(target, system, &text) {
        Ok(answer) => {
            println!("{answer}");
            0
        }
        Err(e) => {
            eprintln!("provider error: {e}");
            3
        }
    }
}

fn cmd_explain() -> i32 {
    let Some(rec) = log::last() else {
        eprintln!("no decisions logged yet — run `maele route \"...\"`");
        return 1;
    };
    let text = rec["text"].as_str().unwrap_or("");
    let d = &rec["decision"];
    println!("┌─────────────────────────────────────┐");
    println!("│ Maele decision                      │");
    println!("├─────────────────────────────────────┤");
    println!("│ Question : {text}");
    println!(
        "│ Layer    : {}   Confidence: {:.2}",
        d["layer"].as_str().unwrap_or("-"),
        d["confidence"].as_f64().unwrap_or(0.0)
    );
    println!(
        "│ Chosen   : {} → {} ({})",
        d["capability"].as_str().unwrap_or("-"),
        d["target"].as_str().unwrap_or("none"),
        d["tier"].as_str().unwrap_or("-")
    );
    if let Some(probs) = d["probabilities"].as_object() {
        for (k, v) in probs {
            println!("│   p({k}) = {:.2}", v.as_f64().unwrap_or(0.0));
        }
    }
    if let Some(err) = d["jev_error"].as_str() {
        println!("│ Jev error: {err}");
    }
    println!("└─────────────────────────────────────┘");
    0
}

fn cmd_doctor(config_path: Option<&Path>) -> i32 {
    let cfg = match load_config(config_path) {
        Ok(c) => c,
        Err(code) => return code,
    };
    println!("config: {}", cfg.source.display());
    println!(
        "layers: {}   fallback: {}   floor: {}",
        cfg.layers.join(", "),
        cfg.fallback,
        cfg.confidence_floor
    );
    if let Some(j) = &cfg.jev {
        println!(
            "jev: {} model={} key={}",
            j.base_url,
            j.model,
            j.key.as_deref().unwrap_or("-")
        );
    } else {
        println!("jev: not configured (routing runs on rules/semantic only)");
    }

    let mut worst = 0;
    println!("\ntargets:");
    for (name, tgt) in &cfg.targets {
        let (ok, note) = providers::health(tgt);
        if !ok {
            worst = 1;
        }
        println!(
            "  {}{:<16} {:<14} {:<28} {}",
            if ok { "ok " } else { "!! " },
            name,
            tgt.kind,
            if tgt.model.is_empty() {
                "-"
            } else {
                &tgt.model
            },
            note
        );
    }

    println!("\ncapabilities:");
    for (name, cap) in &cfg.capabilities {
        let ok = cfg
            .targets
            .get(&cap.target)
            .map(|t| providers::health(t).0)
            .unwrap_or(false);
        println!(
            "  {:<12} → {:<16} {} examples   [{}]",
            name,
            cap.target,
            cap.examples.len(),
            if ok { "ok" } else { "no target" }
        );
    }
    worst
}

fn cmd_keys(action: KeysAction, config_path: Option<&Path>) -> i32 {
    match action {
        KeysAction::Set { service } => {
            eprint!("secret for {service}: ");
            use std::io::Write;
            let _ = std::io::stderr().flush();
            let mut line = String::new();
            if std::io::stdin().read_line(&mut line).is_err() {
                eprintln!("aborted: could not read secret");
                return 1;
            }
            let secret = line.trim();
            if secret.is_empty() {
                eprintln!("aborted: empty secret");
                return 1;
            }
            match keys::keychain_set(&service, secret) {
                Ok(()) => {
                    println!("stored {service} in the login keychain");
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        KeysAction::Check { service } => {
            let found = keys::keychain_get(&service).is_some();
            println!("{service}: {}", if found { "present" } else { "NOT SET" });
            if found {
                0
            } else {
                1
            }
        }
        KeysAction::Delete { service } => {
            println!(
                "{}",
                if keys::keychain_delete(&service) {
                    "deleted"
                } else {
                    "nothing to delete"
                }
            );
            0
        }
        KeysAction::Init { force, path } => write_config(force, path, config_path),
    }
}

fn cmd_config(action: ConfigAction, config_path: Option<&Path>) -> i32 {
    match action {
        ConfigAction::Init { force, path } => write_config(force, path, config_path),
    }
}

fn write_config(force: bool, path: Option<PathBuf>, _config_path: Option<&Path>) -> i32 {
    let dest = path.unwrap_or_else(|| log::dir().join("config.yaml"));
    if dest.exists() && !force {
        eprintln!("{} already exists (use --force)", dest.display());
        return 1;
    }
    if let Some(parent) = dest.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("could not create {}: {e}", parent.display());
            return 1;
        }
    }
    match std::fs::write(&dest, config::EXAMPLE_CONFIG) {
        Ok(()) => {
            println!("wrote {}", dest.display());
            0
        }
        Err(e) => {
            eprintln!("could not write {}: {e}", dest.display());
            1
        }
    }
}
