# Maele

*Mæle* (n.) — voice, speech. "Mål og mæle."

Press a hotkey, ask a question out loud, and the answer comes back from **the
model that should answer it** — not from whichever app happens to be open.

Maele is a voice front end with a routing brain. It does not try to be a
better chatbot. It tries to solve the thing that actually costs you time:
you have five agents, three API keys and a local model, and every question
still gets typed into the wrong one.

```
  ⌥Space (hold) ──▶ local ASR ──▶ ┌───────┐ ──▶ harness + model
                                  │  JEV  │
                                  └───────┘
                                      ▲
                                  config.yaml
```

## The two decisions Jev makes

**1. What does the question need?** — code, research, writing, my own data,
general. Stable, inferable from the text.

**2. How does it need to be answered?** — tone and urgency. A quick factual
check should go to a fast cheap model. A terse frustrated question after a
failed deploy should go to the strong reasoning model with the code harness,
even if the topic alone would have routed it to general chat.

Topic and tone are separate axes. Collapsing them is why routing feels wrong
even when it is technically correct.

## Architecture

| Layer | Choice | Why |
|---|---|---|
| Hotkey + audio + ASR | **Handy** (MIT, Rust + Tauri) | Push-to-talk already solved. Parakeet V3 auto-detects 25 European languages including Norwegian — the NO/EN problem is free. Forkable by design. |
| Routing | **Jev** | Cheap-first: rules → semantic → optional model. Config-driven, no rebuild to change routing. |
| Answering | pluggable targets | Local (Ollama), cloud (OpenAI/Anthropic/OpenRouter), or hand off to an existing harness. |
| UI | Tauri (React) | Inherited from Handy; matches existing skills. |

Why Handy and not a bespoke audio stack: the hotkey listener, device switching,
resampling, model management and text injection are five layers of thankless
work that a 25k-star MIT project already finished. Fork it and replace the
*output* stage — instead of pasting the transcript into the focused app, hand it
to Jev and continue the conversation in the chosen harness.

## Config

```yaml
routing:
  layers: [rules, semantic, tone]
  confidence_floor: 0.35

capabilities:
  code:     { target: anthropic, model: claude-sonnet-4-5 }
  research: { target: openai,    model: gpt-4.1-mini }
  writing:  { target: openai,    model: gpt-4.1-mini }
  mine:     { target: hermes }
  general:  { target: ollama,    model: llama3.2:3b }

tone:
  quick:     { model_tier: small }   # "hva er klokka i Tokyo"
  serious:   { model_tier: large }   # frustrated, long, or consequential
```

Routing a question means picking a capability **and** a tier; the target table
resolves the pair.

## Status

`crates/maele-jev/` is the Rust routing core (Phase 1). Jev — TypeSafe's
System One model — is the brain: one call per voice turn with typed questions
(`capability` Choice, `urgency` Noul, `tone` Score), returning typed answers
with probabilities and confidence. Rules and character-trigram similarity stand
behind it as the fail-open path when Jev is unavailable or abstains.

```bash
cargo test -p maele-jev          # 16 tests
cargo run -p maele-jev -- --config config.example.yaml route --dummy "why is my docker container exiting"
```

`--dummy` routes against `fixtures/dummy_jev.json` with no network, so subtle
NO/EN commands can be exercised offline. `route`, `explain`, `ask`, `doctor`,
`keys`, and `config init` are implemented.

`prototype/jev/` remains the original Python routing core and is the reference
for the Rust port:

```bash
cd prototype
uv venv --python 3.11 && uv pip install -e ".[dev]"
uv run jev route "why is my docker container exiting"
uv run jev doctor
```

`crates/maele-core/` is the audio front end (Phase 2, started). `audio_toolkit`
— cpal recorder, resampler, VAD, language ID, transcript cleanup — is vendored
from Handy's Rust core (MIT; see `LICENSE-HANDY`), Tauri-free. Handy's 2.5k-line
Tauri-coupled transcription manager is deliberately not vendored; Maele owns a
lean ASR wrapper (`asr.rs`) over transcribe.cpp.

The ASR layer is **two engines behind a language gate** (`asr.rs`, over
transcribe.cpp GGUF): a small Whisper (`whisper-small`) detects the spoken
language; Norwegian goes to **NB-Whisper** (National Library of Norway — best
`no`), English goes to **Nemotron Streaming 3.5** (fast, excellent `en`). No
single model covers both: Nemotron's Norwegian is unusable, and NB-Whisper's
auto-detect biases English to Norwegian. Parakeet TDT v3 was ruled out — it does
**not** support Norwegian.

```bash
cargo test -p maele-core                 # 92 tests
cargo run -p maele-core -- devices
cargo run -p maele-core -- fetch         # lid ~270 MB + en ~751 MB + no ~1.1 GB
cargo run -p maele-core -- transcribe --seconds 5 --language auto
cargo run -p maele-core -- push-to-talk  # hold ⌥Space to talk, release to transcribe
```

`--language auto` (default) relies on the gate; `--language no|en` forces an
engine. NB-Whisper large is the quality option but ~1× realtime; swap in
NB-Whisper medium for lower latency. `push-to-talk` needs Microphone and
(usually) Accessibility/Input Monitoring permission for your terminal.

**Toolchain notes:** the GGUF engine runs on transcribe.cpp (ggml + Metal), so it
needs **cmake** but **no ONNX and no Xcode**. The optional Silero VAD backend
(`--features silero`) does pull ONNX Runtime, which needs a **macOS 14+ SDK (full
Xcode)**; it does not link on this machine's CommandLineTools 13.3, so the
default VAD detector is Earshot (pure Rust). GPUI (Phase 3) will also want Xcode.

Not built: hotkey wiring (rest of Phase 2), the GPUI shell (Phase 3), target
kinds `shell` and `shortcut`, and real harness handoff.

## Non-goals

- Being a better assistant than the harness it routes to.
- Replacing a wake word. Maele is push-to-talk on purpose — a hotkey has no
  false positives and no privacy argument.
- App Store distribution. A hotkey + Accessibility API app is a self-signed
  personal build.
