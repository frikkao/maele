"""Language identification and cheap semantic similarity.

Two jobs, no model downloads:

* ``detect_language`` — Norwegian vs English, which is all the NO/EN split needs.
  Norwegian has giveaway function words and the letters æøå that English lacks.
* ``Similarity`` — character-trigram TF-IDF with cosine similarity. A stand-in
  for embeddings that costs microseconds and needs no weights. Good enough to
  decide "this looks like the research examples" when no keyword fired.
"""

from __future__ import annotations

import math
import re
from collections import Counter

_WORD_RE = re.compile(r"[a-zæøåäöü]+", re.IGNORECASE)

# Function words that are near-dispositive for Norwegian Bokmål.
_NO_MARKERS = {
    "og", "ikke", "jeg", "du", "han", "hun", "det", "dette", "disse", "som",
    "er", "var", "blir", "ble", "til", "på", "med", "for", "av", "at", "en",
    "et", "den", "de", "har", "hadde", "kan", "skal", "vil", "må", "bør",
    "hva", "hvordan", "hvor", "når", "hvem", "hvorfor", "meg", "deg", "seg",
    "min", "din", "sin", "vår", "deres", "også", "bare", "men", "eller",
    "fordi", "noe", "noen", "veldig", "ganske", "her", "der", "helt",
    "annet", "hjelpe", "finn", "gjør", "gjorde", "skriv", "les", "hent",
    "vis", "lag", "sett", "kjøp", "send", "trenger", "tror", "vet",
}
_EN_MARKERS = {
    "the", "and", "is", "are", "was", "were", "of", "to", "in", "for", "with",
    "that", "this", "these", "those", "it", "its", "what", "how", "where",
    "when", "who", "why", "my", "your", "our", "their", "not", "but", "or",
    "because", "some", "any", "very", "quite", "here", "there", "help",
    "need", "think", "know", "should", "would", "could", "can", "will",
    "have", "has", "had", "must", "find", "make", "get", "show", "set",
    "buy", "send", "write", "read", "another", "something",
}


def detect_language(text: str) -> str:
    """Return 'nb', 'en', or 'und' when there is not enough signal.

    Norwegian wins ties on the æøå giveaway characters, which English never uses.
    """
    if not text or not text.strip():
        return "und"
    t = text.lower()
    if any(ch in t for ch in "æøå"):
        return "nb"
    words = _WORD_RE.findall(t)
    if not words:
        return "und"
    no_hits = sum(1 for w in words if w in _NO_MARKERS)
    en_hits = sum(1 for w in words if w in _EN_MARKERS)
    if no_hits == en_hits == 0:
        return "und"
    if no_hits > en_hits:
        return "nb"
    if en_hits > no_hits:
        return "en"
    return "und"


def _trigrams(text: str) -> list[str]:
    """Character trigrams over a normalised string, word-boundary padded."""
    t = " " + " ".join(_WORD_RE.findall(text.lower())) + " "
    if len(t) < 3:
        return [t]
    return [t[i : i + 3] for i in range(len(t) - 2)]


class Similarity:
    """TF-IDF over character trigrams, scored by cosine similarity.

    Fit once against the example utterances for each capability.
    """

    def __init__(self) -> None:
        self._vectors: dict[str, dict[str, float]] = {}
        self._norms: dict[str, float] = {}
        self._idf: dict[str, float] = {}
        self._fitted = False

    def fit(self, examples_by_label: dict[str, list[str]]) -> "Similarity":
        docs: dict[str, Counter] = {}
        for label, phrases in examples_by_label.items():
            counts: Counter = Counter()
            for phrase in phrases:
                counts.update(set(_trigrams(phrase)))
            if counts:
                docs[label] = counts
        if not docs:
            self._fitted = False
            return self

        n_docs = len(docs)
        df: Counter = Counter()
        for counts in docs.values():
            for term in counts:
                df[term] += 1
        self._idf = {
            term: math.log((1 + n_docs) / (1 + d)) + 1.0 for term, d in df.items()
        }

        for label, counts in docs.items():
            vec = {
                term: (1 + math.log(c)) * self._idf.get(term, 0.0)
                for term, c in counts.items()
            }
            norm = math.sqrt(sum(v * v for v in vec.values())) or 1.0
            self._vectors[label] = vec
            self._norms[label] = norm
        self._fitted = True
        return self

    @property
    def fitted(self) -> bool:
        return self._fitted

    def scores(self, text: str) -> dict[str, float]:
        """Cosine similarity of ``text`` against each label, in [0, 1]."""
        if not self._fitted or not text.strip():
            return {}
        counts: Counter = Counter()
        for phrase in (text,):
            counts.update(set(_trigrams(phrase)))
        if not counts:
            return {}
        vec = {
            term: (1 + math.log(c)) * self._idf.get(term, 0.0)
            for term, c in counts.items()
            if term in self._idf
        }
        norm = math.sqrt(sum(v * v for v in vec.values()))
        if not norm:
            return {}
        out = {}
        for label, lvec in self._vectors.items():
            dot = sum(v * lvec.get(term, 0.0) for term, v in vec.items())
            out[label] = dot / (norm * self._norms[label])
        return out
