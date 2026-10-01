# Jev

A decision layer. It reads a spoken question, decides **which capability the
question needs**, and resolves that to a configured target — a local model, a
cloud API, or an external harness.

Jev never answers anything itself. That is the point: the routing decision is a
different problem from the answering, and mixing them is how you end up paying
for a frontier model to answer "what time is it".

```
  mic / wake word  →  transcript  →  ┌───────┐  →  the right model
                                     │  JEV  │
                                     └───────┘
```

## Routing layers

Layers run cheapest-first and stop as soon as one clears the confidence floor:

| layer | cost | what it does |
|---|---|---|
| `rules` | microseconds | declared substring triggers |
| `semantic` | ~ms | character-trigram TF-IDF cosine against `examples` |
| `llm` | 100s of ms | optional: a small model arbitrates (opt-in) |
| *fallback* | — | no layer cleared the floor → the `fallback` capability |

Routing to the wrong target is worse than routing slowly, so anything
below the floor falls through rather than guessing.

## Install

```bash
cd ~/git/jev
uv venv --python 3.11
uv pip install -e ".[dev]"
```

## Configure

```bash
uv run jev keys init                 # writes ~/.config/jev/config.yaml
```

Then edit `~/.config/jev/config.yaml`. The interesting parts are `targets`
(where questions can go), `capabilities` (what a question *needs*), and `rules`
(which questions map to which capability).

**Secrets never go in the config.** `key:` holds a reference, resolved at call
time — `keychain:<service>` or `env:<VAR>`:

```bash
uv run jev keys set openrouter       # prompts, stores in the login keychain
uv run jev keys check openrouter
```

## Use

```bash
uv run jev route "why is my docker container exiting"   # where would this go?
uv run jev route "logg dette i idebanken" --json        # full decision object
uv run jev ask   "hva er de siste nyhetene om renten"   # route it and answer it
uv run jev doctor                                       # config + target health
```

`doctor` exits non-zero when any target is unhealthy, so it works as a CI gate.

## Design notes

**Route by capability, not by app.** A question needs *code* or *research* or
*your own data*; which model serves that is a separate, swappable mapping. Swap
ChatGPT for Claude and no rule changes.

**Priority is a tie-breaker, not a weight.** A priority-100 rule with no
matching trigger loses to a priority-60 rule that matched. Priority only orders
rules that both fired.

**A single keyword is a confident signal.** Strength scales with the *number* of
matched triggers (0.60 for one, +0.15 each additional), not with the fraction of
the trigger list that matched. Scaling by fraction makes a rule with 19 triggers
score one keyword at 5% — which silently turns the keyword layer into a semantic
layer and hides the rules you wrote.

**Language detection is a routing input.** `languages: [en]` on a rule stops a
Norwegian phrasing from tripping an English keyword rule.

## Status

Working: config loading + validation, macOS keychain storage, language
detection, all four routing layers, CLI (`route` / `ask` / `doctor` / `keys`),
9 tests.

Not built yet: the audio front end (wake word → transcript), target kinds
`shell` and `shortcut`, and the Mac app shell.
