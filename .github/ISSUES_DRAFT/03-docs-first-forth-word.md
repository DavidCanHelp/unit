---
title: "Docs: 'Your first Forth word in unit' walkthrough"
labels: ["good first issue", "area:docs"]
---

## Summary

The README sells the idea, `docs/words.md` catalogues the 309 primitives,
and the whitepaper covers the theory. There is currently no short,
hands-on walkthrough for a reader who has just run `cargo install unit`,
hit `>`, and is wondering what to type. The closest thing is the embedded
tutorial in the WASM demo, which a CLI-first reader will never see.

This issue is to write `docs/first-word.md`: a 5–10 minute walkthrough
that takes a reader from `2 3 + .` to defining their own Forth word,
inspecting it with `SEE`, listing the dictionary with `WORDS`, and finally
sharing it across two units on the local mesh with `SHARE-ALL` /
`SHARED-WORDS`. The goal is to leave the reader with a working mental
model of what "the dictionary is the genome" actually means.

## Why it's a good first issue

It's a docs PR — no Rust, no WASM, no test surface. But the writer has to
*use* unit to write it, which means a contributor will end up touching
the REPL, the mesh, and the dictionary while producing something that
will help every reader who comes after them.

It's also concrete and finishable in one evening. The shape is fixed:
intro paragraph, a sequence of REPL transcripts with brief commentary
between them, a closing pointer to `docs/words.md` and the demo.

## Acceptance criteria

- [ ] New file: `docs/first-word.md`.
- [ ] Walkthrough covers, in order: arithmetic, defining a word with
      `: NAME ... ;`, calling it, inspecting it with `SEE`, listing the
      dictionary with `WORDS`, and sharing/receiving the word across two
      `unit` processes on loopback.
- [ ] Every transcript block uses real output from running the binary —
      no invented prompts or values.
- [ ] Closes with two pointers: `docs/words.md` for the full reference
      and the live demo URL for readers who don't want to install.
- [ ] Linked from the "Documentation" section in `README.md`.

## Where to look

- `README.md` — tone reference; the "Try It" section is roughly the
  prose register to aim for.
- `docs/words.md` — the catalogue this walkthrough complements.
- `src/prelude.fs` — examples of idiomatic `:`/`;` definitions.
- `justfile` — `just swarm 2` is the easiest way to bring up two units
  on loopback for the sharing demo.

## Nice-to-haves (not required)

- A short "what's next" paragraph pointing at `SPAWN`, `GP-EVOLVE`, and
  the immune system as natural next steps.
