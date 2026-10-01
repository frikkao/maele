//! ASR: two engines behind a language-ID gate.
//!
//! Neither a single multilingual model nor a single Norwegian model covers
//! both languages well on its own, so Maele routes:
//!
//! * a small Whisper (`whisper-base`) detects the spoken language cheaply;
//! * Norwegian goes to **NB-Whisper** (Nasjonalbiblioteket; best `no`);
//! * English goes to **Nemotron Streaming 3.5** (fast, excellent `en`).
//!
//! All inference is `transcribe.cpp` (GGML/GGUF).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use transcribe_cpp::{Model, RunOptions, Session, Task};

/// A model Maele can download.
pub struct ModelSpec {
    pub name: &'static str,
    pub filename: &'static str,
    pub url: &'static str,
}

/// Language-ID model (generic Whisper small; base was too weak on short clips).
pub const LID: ModelSpec = ModelSpec {
    name: "lid",
    filename: "whisper-small-Q8_0.gguf",
    url: "https://huggingface.co/handy-computer/whisper-small-gguf/resolve/main/whisper-small-Q8_0.gguf",
};

/// English engine (Nemotron Streaming 3.5).
pub const EN: ModelSpec = ModelSpec {
    name: "en",
    filename: "nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf",
    url: "https://huggingface.co/handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf/resolve/main/nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf",
};

/// Norwegian engine (NB-Whisper large, National Library of Norway).
pub const NO: ModelSpec = ModelSpec {
    name: "no",
    filename: "nb-whisper-large-q5_0.bin",
    url: "https://huggingface.co/NbAiLab/nb-whisper-large/resolve/main/ggml-model-q5_0.bin",
};

pub const MODELS: &[&ModelSpec] = &[&LID, &EN, &NO];

#[derive(Debug, Clone)]
pub struct Transcription {
    pub text: String,
    pub raw_text: String,
    pub language: Option<String>,
    pub engine: String,
}

/// A loaded model with a reusable session.
pub struct Asr {
    session: Session,
    engine: String,
}

impl Asr {
    pub fn load(name: &str, path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let model = Model::load(path)
            .with_context(|| format!("loading {name} model {}", path.display()))?;
        let session = model.session().context("creating ASR session")?;
        Ok(Self {
            session,
            engine: name.to_string(),
        })
    }

    /// Transcribe 16 kHz mono samples. `language` is `None` for auto-detect.
    pub fn transcribe(
        &mut self,
        pcm: &[f32],
        language: Option<&str>,
        keep_special_tags: bool,
    ) -> Result<Transcription> {
        let out = self.run(pcm, language, keep_special_tags)?;
        Ok(Transcription {
            text: out.text,
            raw_text: out.raw_text,
            language: out.language,
            engine: self.engine.clone(),
        })
    }

    /// Detect the spoken language only (runs the generic Whisper with no hint).
    /// Capped to the first few seconds: language identification does not need
    /// the whole utterance, and decoding all of it doubles latency.
    pub fn detect_language(&mut self, pcm: &[f32]) -> Result<Option<String>> {
        const LID_MAX_SAMPLES: usize = 3 * 16_000;
        let pcm = if pcm.len() > LID_MAX_SAMPLES {
            &pcm[..LID_MAX_SAMPLES]
        } else {
            pcm
        };
        let out = self.run(pcm, None, true)?;
        Ok(normalize_detected(
            out.language.as_deref().or_else(|| tag_in(&out.raw_text)),
        ))
    }

    fn run(
        &mut self,
        pcm: &[f32],
        language: Option<&str>,
        keep_special_tags: bool,
    ) -> Result<transcribe_cpp::Transcript> {
        let options = RunOptions {
            task: Task::Transcribe,
            language: language.map(str::to_string),
            keep_special_tags,
            ..Default::default()
        };
        self.session
            .run(pcm, &options)
            .context("running transcription")
    }
}

/// The two-engine import: detect the language, then dispatch.
pub struct Bilingual {
    lid: Asr,
    en: Asr,
    no: Asr,
}

impl Bilingual {
    pub fn load(lid: &Path, en: &Path, no: &Path) -> Result<Self> {
        Ok(Self {
            lid: Asr::load("lid", lid)?,
            en: Asr::load("en", en)?,
            no: Asr::load("no", no)?,
        })
    }

    pub fn from_default_paths() -> Result<Self> {
        Self::load(&default_path(&LID), &default_path(&EN), &default_path(&NO))
    }

    /// Transcribe, honouring a forced language (`"auto"`/`None` = detect).
    ///
    /// Maele is a NO/EN tool, so the gate is deliberately one-sided: only a
    /// confident `en` goes to the English engine, and everything else
    /// (including `nn`, and the odd `pt`/`da` mis-detection on short clips)
    /// goes to NB-Whisper. This trades a rare English-as-other mis-route for
    /// never sending Norwegian to the engine that cannot read it.
    pub fn transcribe(
        &mut self,
        pcm: &[f32],
        force: Option<&str>,
        keep_special_tags: bool,
    ) -> Result<Transcription> {
        let english = match force {
            Some(l) if !l.eq_ignore_ascii_case("auto") => l.to_lowercase().starts_with("en"),
            _ => match self.lid.detect_language(pcm)? {
                Some(l) => l.starts_with("en"),
                None => false,
            },
        };
        if english {
            self.en.transcribe(pcm, Some("en-US"), keep_special_tags)
        } else {
            self.no.transcribe(pcm, Some("no"), keep_special_tags)
        }
    }
}

/// Normalise a detector's language code to `"no"`/`"en"`, or pass it through.
fn normalize_detected(lang: Option<&str>) -> Option<String> {
    let l = lang?.trim().to_lowercase();
    if l.is_empty() {
        return None;
    }
    if l.starts_with("no") || l.starts_with("nb") || l.starts_with("nn") || l == "nob" || l == "nno"
    {
        Some("no".into())
    } else if l.starts_with("en") {
        Some("en".into())
    } else {
        Some(l)
    }
}

/// Extract a `<xx-XX>` / `<xx>` language tag from a raw transcript.
fn tag_in(raw: &str) -> Option<&str> {
    let start = raw.rfind('<')?;
    let end = raw[start..].find('>')? + start;
    let inner = raw[start + 1..end].trim();
    if inner.is_empty() || inner.contains('|') {
        None
    } else {
        Some(inner)
    }
}

// -- model paths + download --------------------------------------------------

pub fn models_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().join(".config/maele/models"))
        .unwrap_or_else(|| PathBuf::from(".maele/models"))
}

pub fn default_path(spec: &ModelSpec) -> PathBuf {
    models_dir().join(spec.filename)
}

/// Download every model Maele needs, skipping any already present.
pub fn fetch_all() -> Result<()> {
    for spec in MODELS {
        fetch(spec)?;
    }
    Ok(())
}

/// Download one model into [`models_dir`], with progress.
pub fn fetch(spec: &ModelSpec) -> Result<PathBuf> {
    use std::io::{Read, Write};

    let dir = models_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let dest = dir.join(spec.filename);
    if dest.exists() {
        println!("{}: already present ({})", spec.name, dest.display());
        return Ok(dest);
    }

    println!("{}: downloading {}", spec.name, spec.url);
    let mut resp = reqwest::blocking::get(spec.url)
        .with_context(|| format!("requesting {}", spec.url))?
        .error_for_status()
        .with_context(|| format!("downloading {}", spec.url))?;
    let total = resp.content_length();

    let tmp = dest.with_extension("partial");
    let mut file =
        std::fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    let mut buf = [0u8; 1 << 16];
    let mut done: u64 = 0;
    let mut last_pct = u64::MAX;
    loop {
        let n = resp.read(&mut buf).context("reading response")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).context("writing model")?;
        done += n as u64;
        let pct = total.map(|t| done * 100 / t.max(1));
        if pct != Some(last_pct) {
            match total {
                Some(t) => println!(
                    "  {:>5.1}%  {:.0}/{:.0} MB",
                    done as f64 * 100.0 / t as f64,
                    done as f64 / 1e6,
                    t as f64 / 1e6
                ),
                None => println!("  {:.0} MB", done as f64 / 1e6),
            }
            last_pct = pct.unwrap_or(u64::MAX);
        }
    }
    file.sync_all().ok();
    drop(file);
    std::fs::rename(&tmp, &dest).with_context(|| format!("finalising {}", dest.display()))?;
    println!("{}: wrote {}", spec.name, dest.display());
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::{normalize_detected, tag_in};

    #[test]
    fn normalizes_norwegian_variants() {
        for code in ["no", "nb", "nn", "nb-NO", "nob", "nno"] {
            assert_eq!(
                normalize_detected(Some(code)).as_deref(),
                Some("no"),
                "{code}"
            );
        }
    }

    #[test]
    fn normalizes_english_and_passes_through_unknown() {
        assert_eq!(normalize_detected(Some("en")).as_deref(), Some("en"));
        assert_eq!(normalize_detected(Some("en-US")).as_deref(), Some("en"));
        // A short-clip mis-detection must survive normalisation so the
        // one-sided gate can send it to Norwegian.
        assert_eq!(normalize_detected(Some("pt")).as_deref(), Some("pt"));
        assert_eq!(normalize_detected(Some("  ")), None);
        assert_eq!(normalize_detected(None), None);
    }

    #[test]
    fn extracts_language_tag_from_raw_text() {
        assert_eq!(tag_in("Why is this? <en-US>"), Some("en-US"));
        assert_eq!(tag_in("no tag here"), None);
        // Special vocab tags like <|notimestamps|> are not languages.
        assert_eq!(tag_in("text <|endoftext|>"), None);
    }
}
