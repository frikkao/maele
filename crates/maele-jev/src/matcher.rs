//! Language identification and cheap semantic similarity.
//!
//! Two jobs, no model downloads:
//!
//! * [`detect_language`] — Norwegian vs English. Norwegian has giveaway
//!   function words and the letters æøå that English lacks.
//! * [`Similarity`] — character-trigram TF-IDF with cosine similarity. A
//!   stand-in for embeddings that costs microseconds and needs no weights.

use std::collections::{HashMap, HashSet};

const NO_MARKERS: &[&str] = &[
    "og", "ikke", "jeg", "du", "han", "hun", "det", "dette", "disse", "som", "er", "var", "blir",
    "ble", "til", "på", "med", "for", "av", "at", "en", "et", "den", "de", "har", "hadde", "kan",
    "skal", "vil", "må", "bør", "hva", "hvordan", "hvor", "når", "hvem", "hvorfor", "meg", "deg",
    "seg", "min", "din", "sin", "vår", "deres", "også", "bare", "men", "eller", "fordi", "noe",
    "noen", "veldig", "ganske", "her", "der", "helt", "annet", "hjelpe", "finn", "gjør", "gjorde",
    "skriv", "les", "hent", "vis", "lag", "sett", "kjøp", "send", "trenger", "tror", "vet",
];

const EN_MARKERS: &[&str] = &[
    "the",
    "and",
    "is",
    "are",
    "was",
    "were",
    "of",
    "to",
    "in",
    "for",
    "with",
    "that",
    "this",
    "these",
    "those",
    "it",
    "its",
    "what",
    "how",
    "where",
    "when",
    "who",
    "why",
    "my",
    "your",
    "our",
    "their",
    "not",
    "but",
    "or",
    "because",
    "some",
    "any",
    "very",
    "quite",
    "here",
    "there",
    "help",
    "need",
    "think",
    "know",
    "should",
    "would",
    "could",
    "can",
    "will",
    "have",
    "has",
    "had",
    "must",
    "find",
    "make",
    "get",
    "show",
    "set",
    "buy",
    "send",
    "write",
    "read",
    "another",
    "something",
];

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, 'æ' | 'ø' | 'å' | 'ä' | 'ö' | 'ü')
}

fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if is_word_char(c) {
            for lc in c.to_lowercase() {
                cur.push(lc);
            }
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Return `"nb"`, `"en"`, or `"und"` when there is not enough signal.
///
/// Norwegian wins ties on the æøå giveaway characters, which English never uses.
pub fn detect_language(text: &str) -> &'static str {
    if text.trim().is_empty() {
        return "und";
    }
    let t = text.to_lowercase();
    if t.chars().any(|c| matches!(c, 'æ' | 'ø' | 'å')) {
        return "nb";
    }
    let ws = words(&t);
    if ws.is_empty() {
        return "und";
    }
    let no_set: HashSet<&str> = NO_MARKERS.iter().copied().collect();
    let en_set: HashSet<&str> = EN_MARKERS.iter().copied().collect();
    let no_hits = ws.iter().filter(|w| no_set.contains(w.as_str())).count();
    let en_hits = ws.iter().filter(|w| en_set.contains(w.as_str())).count();
    if no_hits == 0 && en_hits == 0 {
        return "und";
    }
    if no_hits > en_hits {
        "nb"
    } else if en_hits > no_hits {
        "en"
    } else {
        "und"
    }
}

/// Character trigrams over a normalised string, word-boundary padded.
fn trigrams(text: &str) -> Vec<String> {
    let joined = words(text).join(" ");
    let t = format!(" {} ", joined.to_lowercase());
    let chars: Vec<char> = t.chars().collect();
    if chars.len() < 3 {
        return vec![t];
    }
    chars
        .windows(3)
        .map(|w| w.iter().collect::<String>())
        .collect()
}

/// TF-IDF over character trigrams, scored by cosine similarity.
///
/// Fit once against the example utterances for each capability.
#[derive(Debug, Default)]
pub struct Similarity {
    vectors: HashMap<String, HashMap<String, f64>>,
    norms: HashMap<String, f64>,
    idf: HashMap<String, f64>,
    fitted: bool,
}

impl Similarity {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fit(mut self, examples_by_label: &HashMap<String, Vec<String>>) -> Self {
        let mut docs: HashMap<String, HashMap<String, f64>> = HashMap::new();
        for (label, phrases) in examples_by_label {
            let mut counts: HashMap<String, f64> = HashMap::new();
            for phrase in phrases {
                for tri in trigrams(phrase).into_iter().collect::<HashSet<_>>() {
                    *counts.entry(tri).or_insert(0.0) += 1.0;
                }
            }
            if !counts.is_empty() {
                docs.insert(label.clone(), counts);
            }
        }
        if docs.is_empty() {
            self.fitted = false;
            return self;
        }

        let n_docs = docs.len() as f64;
        let mut df: HashMap<String, f64> = HashMap::new();
        for counts in docs.values() {
            for term in counts.keys() {
                *df.entry(term.clone()).or_insert(0.0) += 1.0;
            }
        }
        self.idf = df
            .into_iter()
            .map(|(term, d)| (term, ((1.0 + n_docs) / (1.0 + d)).ln() + 1.0))
            .collect();

        for (label, counts) in docs {
            let mut vec: HashMap<String, f64> = HashMap::new();
            for (term, c) in counts {
                let idf = self.idf.get(&term).copied().unwrap_or(0.0);
                vec.insert(term, (1.0 + c.ln()) * idf);
            }
            let norm = vec.values().map(|v| v * v).sum::<f64>().sqrt();
            self.norms
                .insert(label.clone(), if norm == 0.0 { 1.0 } else { norm });
            self.vectors.insert(label, vec);
        }
        self.fitted = true;
        self
    }

    pub fn fitted(&self) -> bool {
        self.fitted
    }

    /// Cosine similarity of `text` against each label, in `[0, 1]`.
    pub fn scores(&self, text: &str) -> HashMap<String, f64> {
        let mut out = HashMap::new();
        if !self.fitted || text.trim().is_empty() {
            return out;
        }
        let counts: HashSet<String> = trigrams(text).into_iter().collect();
        if counts.is_empty() {
            return out;
        }
        let mut vec: HashMap<String, f64> = HashMap::new();
        for term in counts {
            if let Some(idf) = self.idf.get(&term) {
                vec.insert(term, *idf);
            }
        }
        let norm = vec.values().map(|v| v * v).sum::<f64>().sqrt();
        if norm == 0.0 {
            return out;
        }
        for (label, lvec) in &self.vectors {
            let dot: f64 = vec
                .iter()
                .map(|(term, v)| v * lvec.get(term).copied().unwrap_or(0.0))
                .sum();
            let lnorm = self.norms.get(label).copied().unwrap_or(1.0);
            out.insert(label.clone(), dot / (norm * lnorm));
        }
        out
    }
}
