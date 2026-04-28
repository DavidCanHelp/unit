---
title: "Improve stack-underflow error messages in arithmetic primitives"
labels: ["good first issue", "area:core"]
---

## Summary

Forth primitives like `+`, `-`, `*`, `/`, and `MOD` produce a terse error
when the data stack doesn't have enough operands. For someone who just
typed their first definition wrong, the current message is not very
self-explanatory — they don't know whether they're missing one item or
three, or which word triggered the underflow.

This issue is to improve those messages so they read more like
"`+` needs 2 items, stack had 1 (in word `SQUARE`)" — naming the word and
the shortfall.

## Why it's a good first issue

The change is local to `src/vm/` (likely the arithmetic primitives module
plus wherever the underflow error is constructed). No cross-module design
decisions, no protocol changes, no new dependencies. It's a great way to
get familiar with how the Forth VM dispatches primitives and how errors
flow back to the REPL.

It's also user-visible: anyone running `cargo install unit` and typing at
the REPL will benefit on day one.

## Acceptance criteria

- [ ] At least the binary arithmetic words (`+`, `-`, `*`, `/`, `MOD`) and
      the comparison words (`=`, `<`, `>`) report a clearer underflow
      message that names the operand count and, when available, the
      enclosing word.
- [ ] The new message text is captured in a unit test (under `src/vm/` or
      `tests/`) so future regressions are caught.
- [ ] No change to successful behavior — existing tests still pass.
- [ ] WASM REPL output matches native (verify by running the demo).

## Where to look

- `src/vm/` — interpreter and primitive implementations
- `src/integration_tests.rs` — pattern for stack/error tests
- Recent fix-pass commits for tone reference: `4696b6f`, `92c2e30`,
  `1855f24`
