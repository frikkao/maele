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

`prototype/jev/` is a working routing core (999 lines, 9 tests passing) — rules,
semantic (character-trigram TF-IDF, no model download), optional LLM layer,
language detection, macOS keychain for secrets, and a CLI:

```bash
cd prototype
uv venv --python 3.11 && uv pip install -e ".[dev]"
uv run jev route "why is my docker container exiting"
uv run jev doctor
```

Not built: the Handy fork, the tone axis, the Tauri UI, target kinds `shell`
and `shortcut`.

## Non-goals

- Being a better assistant than the harness it routes to.
- Replacing a wake word. Maele is push-to-talk on purpose — a hotkey has no
  false positives and no privacy argument.
- App Store distribution. A hotkey + Accessibility API app is a self-signed
  personal build.
