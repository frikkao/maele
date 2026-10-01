//! `maele-asr` — headless audio front end.
//!
//! `devices` lists inputs; `listen` records a WAV; `fetch` downloads models;
//! `transcribe` records (or reads a WAV) and transcribes it. Without `--model`
//! it uses the two-engine pipeline: LID → NB-Whisper (no) / Nemotron (en).

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use maele_core::{
    fetch, fetch_all, list_input_devices, read_wav_samples, save_wav_file, Asr, AudioRecorder,
    Bilingual, Transcription, VadPolicy, MODELS,
};

#[derive(Parser, Debug)]
#[command(name = "maele-asr", about = "Maele audio front end")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List input devices
    Devices,
    /// Record for a fixed number of seconds and write a WAV
    Listen {
        #[arg(long, default_value_t = 3)]
        seconds: u64,
        #[arg(long, default_value = "maele-capture.wav")]
        out: String,
    },
    /// Download the language-ID, English, and Norwegian models
    Fetch {
        /// Download only one model: lid | en | no
        #[arg(long)]
        only: Option<String>,
    },
    /// Record (or read a WAV) and transcribe it
    Transcribe {
        /// Single model override; otherwise the bilingual pipeline is used
        #[arg(long)]
        model: Option<PathBuf>,
        /// Read samples from a WAV instead of recording
        #[arg(long)]
        wav: Option<PathBuf>,
        /// Seconds to record when --wav is not given
        #[arg(long, default_value_t = 5)]
        seconds: u64,
        /// "auto", "no" (nb/nn), or "en"
        #[arg(long, default_value = "auto")]
        language: String,
        /// Keep special tags (e.g. <en-US>) and print raw_text
        #[arg(long)]
        raw: bool,
    },
    /// Push-to-talk daemon: hold Opt+Space to record, release to transcribe
    PushToTalk {
        /// "auto", "no", or "en"
        #[arg(long, default_value = "auto")]
        language: String,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().cmd {
        Command::Devices => {
            let devices = list_input_devices()?;
            if devices.is_empty() {
                println!("no input devices found (check microphone permission for your terminal)");
            }
            for d in devices {
                println!(
                    "  {}{}  {}",
                    if d.is_default { "* " } else { "  " },
                    d.index,
                    d.name
                );
            }
        }
        Command::Listen { seconds, out } => {
            let samples = record(seconds)?;
            let secs = samples.len() as f64 / 16_000.0;
            save_wav_file(&out, &samples)?;
            println!("wrote {out} ({} samples, {secs:.2}s)", samples.len());
        }
        Command::Fetch { only } => match only {
            Some(name) => {
                let spec = MODELS
                    .iter()
                    .find(|m| m.name == name)
                    .ok_or_else(|| format!("unknown model '{name}' (try: lid, en, no)"))?;
                fetch(spec)?;
            }
            None => fetch_all()?,
        },
        Command::Transcribe {
            model,
            wav,
            seconds,
            language,
            raw,
        } => {
            let samples = match wav {
                Some(p) => read_wav_samples(&p)?,
                None => record(seconds)?,
            };
            let (out, infer) = match model {
                Some(path) => {
                    println!("loading {} …", path.display());
                    let mut asr = Asr::load("model", &path)?;
                    let lang = if language == "auto" {
                        None
                    } else {
                        Some(language.as_str())
                    };
                    let t = std::time::Instant::now();
                    let out = asr.transcribe(&samples, lang, raw)?;
                    (out, t.elapsed())
                }
                None => {
                    println!("loading bilingual pipeline …");
                    let mut bi = Bilingual::from_default_paths()?;
                    let t = std::time::Instant::now();
                    let out = bi.transcribe(&samples, Some(language.as_str()), raw)?;
                    (out, t.elapsed())
                }
            };
            report(&out, &samples, infer.as_secs_f64(), raw);
        }
        Command::PushToTalk { language } => push_to_talk(language)?,
    }
    Ok(())
}

/// Hold Opt+Space to record; release to transcribe. Models load once, so the
/// Metal kernels stay warm for every subsequent turn.
fn push_to_talk(language: String) -> Result<(), Box<dyn std::error::Error>> {
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    use global_hotkey::GlobalHotKeyManager;

    // The manager must be created on the main thread, and macOS delivers its
    // events from the main run loop; the worker below does the heavy lifting.
    let manager = GlobalHotKeyManager::new()?;
    let hotkey = HotKey::new(Some(Modifiers::ALT), Code::Space);
    manager.register(hotkey)?;
    println!("maele: ready — hold ⌥Space to talk, release to transcribe");

    std::thread::spawn(move || worker(language));

    run_event_loop();
    drop(manager);
    Ok(())
}

fn worker(language: String) {
    let mut pipeline = match Bilingual::from_default_paths() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("maele: could not load models: {e} — run `maele-asr fetch`");
            return;
        }
    };
    let mut recorder = match AudioRecorder::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("maele: recorder init failed: {e}");
            return;
        }
    };

    let rx = global_hotkey::GlobalHotKeyEvent::receiver();
    let mut recording = false;
    loop {
        let Ok(event) = rx.recv() else { continue };
        match event.state {
            global_hotkey::HotKeyState::Pressed if !recording => {
                if let Err(e) = recorder.open(None) {
                    eprintln!("maele: could not open mic: {e}");
                    continue;
                }
                if let Ok(ready) = recorder.start(VadPolicy::Disabled) {
                    let _ = ready.recv();
                    recording = true;
                    println!("● recording…");
                }
            }
            global_hotkey::HotKeyState::Released if recording => {
                recording = false;
                let samples = match recorder.stop() {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("maele: stop failed: {e}");
                        continue;
                    }
                };
                let secs = samples.len() as f64 / 16_000.0;
                match pipeline.transcribe(&samples, Some(&language), false) {
                    Ok(out) => println!("▸ [{} | {secs:.2}s] {}", out.engine, out.text),
                    Err(e) => eprintln!("maele: transcribe failed: {e}"),
                }
            }
            _ => {}
        }
    }
}

/// Run the platform event loop on the main thread so the hotkey manager
/// receives events. macOS uses CFRunLoop; other platforms block.
#[cfg(target_os = "macos")]
fn run_event_loop() {
    core_foundation::runloop::CFRunLoop::run_current();
}

#[cfg(not(target_os = "macos"))]
fn run_event_loop() {
    loop {
        std::thread::park();
    }
}

fn report(out: &Transcription, samples: &[f32], infer_s: f64, raw: bool) {
    let secs = samples.len() as f64 / 16_000.0;
    println!(
        "\n[{} | {:.2}s audio, {:.2}s infer | lang={}]",
        out.engine,
        secs,
        infer_s,
        out.language.as_deref().unwrap_or("?")
    );
    if raw {
        println!("raw: {}", out.raw_text);
    }
    println!("{}", out.text);
}

fn record(seconds: u64) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let mut recorder = AudioRecorder::new()?;
    recorder.open(None)?;
    let ready = recorder.start(VadPolicy::Disabled)?;
    ready.recv()?;
    println!("recording {seconds}s …");
    std::thread::sleep(Duration::from_secs(seconds));
    let samples = recorder.stop()?;
    recorder.close()?;
    Ok(samples)
}
