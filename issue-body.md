# Maele — routing a voice question to the right harness and model

## Concept

A macOS app: hold a hotkey, ask a question out loud, release. The transcript is
handed to **Jev**, which reads a user config and decides which **agent harness**
continues the conversation, with which **model** — chosen from the topic *and*
the tone of the question.

The user supplies API keys for Jev and for the harnesses they want in play. The
config — not the code — decides where any given question lands.

```
  ⌥Space (hold to talk)
        │
        ▼
  local ASR (Parakeet V3, on-device, auto language detect)
        │  transcript
        ▼
  ┌─────────────┐
  │     JEV     │  ← config.yaml (targets, capabilities, rules, tone)
  └─────────────┘
        │  route decision
        ▼
  chosen harness + model  ──▶  answer continues there
```

## Why this exists

Five agents, three API keys, one local model — and every question still gets
typed into whichever one happens to be open. A frontier model gets paid to
answer "what's the capital of Australia" while the actual debugging question
goes to a 3B local model. The routing decision is a separate problem from the
answering, and mixing them is the cost.

## Two routing axes

**Capability (topic)** — code, research, writing, my own data, general.
Stable and inferable from the text.

**Tone / urgency** — a quick factual check should go to a fast cheap model; a
terse question after a failed deploy should go to the strong reasoning model
even if the topic alone would have routed it to chat. Signals: length, terse
imperatives, frustration markers, question form ("what is X" vs "why is X
still broken").

Conflating these is why routing feels wrong even when it is technically
correct.

## Stack decisions

| Layer | Choice | Rationale |
|---|---|---|
| Hotkey + audio + ASR | **Fork of Handy** (MIT, Rust + Tauri v2, ~25k★) | Push-to-talk, device switching, resampling, model management and text injection are five layers of thankless work already finished. Replace the *output* stage: instead of pasting into the focused app, hand the transcript to Jev. |
| Language detection NO/EN | **Parakeet V3** (Handy's default) | Auto-detects 25 European languages including Norwegian — removes the custom language-sensor work entirely. |
| Routing | **Jev** | Cheap-first layers. Config-driven; no rebuild to change routing. |
| Secrets | **macOS login keychain** | `keychain:<service>` references in config. Keys never in a file. |
| Answer path | pluggable targets | Ollama (local), OpenAI / Anthropic / OpenRouter, or hand off to an existing harness (Hermes, Shortcuts). |

Handy is MIT and explicitly built to be "the most forkable speech-to-text app",
which makes it the right base rather than a dependency.

## Routing layers (cheapest first, stop on first confident)

| Layer | Cost | Mechanism |
|---|---|---|
| `rules` | µs | declared substring triggers |
| `semantic` | ~ms | character-trigram TF-IDF cosine against `examples` — no model download |
| `tone` | ~ms | tone/urgency classifier, picks the model tier |
| `llm` | 100s of ms | optional: a small model arbitrates when everything else abstains |
| *fallback* | — | nothing cleared the floor → `fallback` capability, never a guess |

Routing to the wrong harness is worse than routing slowly, so anything below
`confidence_floor` falls through rather than guessing.

## Config shape

```yaml
routing:
  layers: [rules, semantic, tone]
  confidence_floor: 0.35

targets:
  anthropic: { kind: anthropic, model: claude-sonnet-4-5, key: keychain:anthropic }
  openai:    { kind: openai_compat, base_url: https://api.openai.com/v1,
               model: gpt-4.1-mini, key: keychain:openai }
  ollama:    { kind: ollama, base_url: http://localhost:11434/v1, model: llama3.2:3b }
  hermes:    { kind: openai_compat, base_url: http://127.0.0.1:8642/v1, model: hermes }

capabilities:
  code:     { target: anthropic, examples: ["why is my docker container exiting"] }
  research: { target: openai,    examples: ["hva er de siste nyhetene om renten"] }
  mine:     { target: hermes,    examples: ["hva har jeg på kalenderen i morgen"] }
  general:  { target: ollama,    examples: ["forklar hvordan en varmepumpe fungerer"] }

tone:
  quick:   { model_tier: small }   # short factual, "what is X"
  serious: { model_tier: large }   # frustrated, long, or consequential
```

A route decision = (capability, tier). The target table resolves the pair, so
swapping ChatGPT for Claude changes one line and no rules.

## What already exists

`prototype/jev/` in this repo — a working routing core:

- ✅ config load + validation (rejects capabilities pointing at unknown targets)
- ✅ macOS keychain storage for API keys
- ✅ NO/EN language detection (marker words + æøå), used as a rule filter
- ✅ rules layer, semantic layer, optional LLM layer, confidence floor, fallback
- ✅ CLI: `route` / `ask` / `doctor` / `keys`
- ✅ 9 tests passing

Verified routing output:

```
logg dette i idebanken                        mine      rules     0.78  nb  -> hermes
why is my docker container exiting with 137   code      rules     0.62  en  -> anthropic
hva er de siste nyhetene om renten            research  rules     0.60  nb  -> openai
hva har jeg på kalenderen i morgen            mine      semantic  0.60  nb  -> hermes
kan du hjelpe meg med noe helt annet          general   fallback  0.00  nb  -> openrouter
```

Two bugs found by running it: rule strength scaled by *fraction* of triggers
matched (a 19-trigger rule scored one keyword at 5% and silently became a
semantic layer), and short Norwegian sentences were detected as unknown
language. Both fixed.

## Open questions

1. **Tone as a routing axis** — is a heuristic classifier enough (length, question
   form, frustration markers), or does tone need a model? A model per utterance
   adds latency to every question to serve a minority.
2. **Harness handoff** — when Jev routes to Hermes or ChatGPT, does Maele (a) call
   its API directly, (b) open the app via URL scheme and paste, or (c) continue
   in Maele's own window? (a) is cleanest, (c) is most controllable.
3. **Fork vs. plugin** — fork Handy outright, or build a separate hotkey app and
   keep Handy as the ASR library? Forking inherits the ASR work but means
   tracking upstream (97 open PRs, active).
4. **Distribution** — self-signed personal build only? TCC keying means any
   signature change drops the mic and Accessibility grants, so "build it
   yourself" needs to be a documented path.
5. **NO/EN rules** — do routing rules need to be language-tagged, or should there
   be a separate ruleset per language? Currently handled with a `languages:`
   filter per rule.
6. **Feedback loop** — log every decision (question, layer, confidence, and
   whether the user re-asked a different harness). That log is the tuning data.

## Non-goals

- Being a better assistant than the harness it routes to.
- A wake word. Push-to-talk on purpose: no false positives, no always-on mic.
- App Store distribution.
- Mobile.

## Milestones

- [ ] **M1 — Router (done)** — Jev core with rules + semantic + fallback + CLI.
- [ ] **M2 — Tone layer** — tone classifier picks the model tier; tests for
      quick vs. serious phrasing in both languages.
- [ ] **M3 — Fork Handy** — replace the text-injection output stage with a Jev
      call; config file drives routing; keychain for keys.
- [ ] **M4 — Handoff** — route to a remote harness and return the answer to
      Maele's own window.
- [ ] **M5 — Decision log** — persist every routing decision as tuning data.
