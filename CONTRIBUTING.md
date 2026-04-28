# Contributing to unit

unit welcomes all forms of contribution — new Forth words, new evolutionary
mechanisms, performance work, WASM compatibility fixes, visualization
improvements, language bindings, docs, bug reports, design discussion. The
project sits at the intersection of artificial-life research and Rust
systems work; whether you arrived from the ALIFE paper or from a hacker-news
thread about a 1.2 MB Forth interpreter, you are equally welcome here.

If you're not sure where your idea fits, open an issue and ask. Most things
that sound interesting probably are.

## What unit is

unit is a self-replicating software nanobot: a minimal Forth interpreter
that is also a networked mesh agent. The dictionary is the genome, the
interpreter is the metabolism, UDP gossip is the voice, and `SPAWN` is
reproduction. The kernel — Forth VM, mesh protocol, replication, mutation
engine, persistence — is about 2,000 lines of Rust with zero external
dependencies. Everything else (immune system, metabolic energy, sexual
reproduction, niche construction, three-order meta-evolution) emerged from
asking what a self-improving organism should do next.

From the ALife side: this is a substrate where genotype and phenotype are
the same object, evolution runs continuously, and three species (Rust, Go,
Python) coexist on a shared S-expression mesh. From the systems side: it's
a hand-rolled Forth + UDP gossip + GP engine with no `unsafe` outside the
WASM boundary, no async runtime, no JSON crate. See the
[README](README.md), [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), and the
[whitepaper](docs/unit-whitepaper-2026.pdf) for depth.

## Getting set up

```sh
git clone https://github.com/DavidCanHelp/unit
cd unit
cargo build --release
cargo test
just wasm        # build the browser demo target
python3 -m http.server -d web 8000  # serve the demo at http://localhost:8000
```

`just --list` shows the rest. `just ci` runs what GitHub Actions runs
(fmt-check + clippy + tests).

## The shape of a contribution

Branch from `main`. Keep PRs focused — one concern per PR is easier to
review than five. Add a test where it makes sense; the existing suite
(`cargo test`, ~255 Rust tests) is the safety net. Run `just ci` before
opening the PR.

A few area-specific notes:

- **New Forth words.** Implement in the relevant `src/vm/` module, add to
  `src/prelude.fs` if it's a high-level word, and update
  [docs/words.md](docs/words.md). If the word touches platform features
  (filesystem, networking, sleep), check `src/wasm_entry.rs` and
  `web/unit.js` — the browser ships shims for non-portable words, and a
  silent WASM regression there is the most common kind of "passes CI,
  breaks the demo." Recent commits with the `WASM shims` prefix in `git
  log` show what that work tends to look like.
- **Evolution mechanics.** If you change `evolve.rs`, `landscape.rs`,
  `niche.rs`, or `reproduction.rs`, a short note in `docs/` describing the
  intent (and any new emergent behavior you observed) is appreciated.
  These modules compose, and "what changed and why" is harder to recover
  from a diff than from a paragraph.
- **Performance work.** Numbers help. The mesh has bench scaffolding (see
  the `O(k) gossip dispatch` commit) and the metrics module exposes
  histograms. Compare before/after on the same hardware.
- **Browser demo.** The `web/` directory is plain HTML/JS — no build step,
  no framework, no dependencies. Ship the rebuilt `web/unit.wasm` when
  your change requires it. Verify in a real browser before opening the PR.

## Communication

Issues for bugs and concrete proposals — what you saw, what you expected,
what you're proposing. Discussions of design, research direction, or "what
if unit could…" are also welcome as issues; tag them `discussion` so they're
easy to find. Pull requests are also a fine place to start a conversation
if a code sketch communicates the idea better than prose.

There is no Slack, Discord, or chat. The project is async by design.

## Closing

unit is in active research-grade development. APIs may shift between minor
versions. Reviews are usually within a few days but not always within a
day; if a PR has been quiet for a week, a polite ping is welcome. Be kind
in reviews and in issues. That's the whole code of conduct.
