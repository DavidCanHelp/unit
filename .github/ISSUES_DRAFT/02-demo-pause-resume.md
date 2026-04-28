---
title: "Browser demo: add a Pause/Resume affordance"
labels: ["good first issue", "area:wasm"]
---

## Summary

The WASM demo at <https://davidcanhelp.github.io/unit/> is now alife-first:
a lone unit calls "Hello?", spawns a friend, and the colony chatters
sparsely from there. This is great for landing impressions, but a visitor
who wants to read a long bubble, take a screenshot, or just stop and look
has no way to freeze the canvas.

This issue is to add a small "pause" chip to the header that suspends the
autonomous timers (`autoTick`, `autoSpawnCheck`, `loneChatterTick`) and a
"resume" toggle that brings them back. Existing simulation state (units,
fitness, energy, dictionary) should be preserved across the pause — only
the autonomous *drivers* stop. The REPL should remain usable while paused.

## Why it's a good first issue

Self-contained: lives entirely inside `web/index.html` and touches no
Rust, no WASM, no protocol. The autonomous timers are all `setInterval`
calls in one section of the file (search for `setInterval(autoTick`),
which makes the pause point obvious. No deep simulation knowledge needed.

It also leaves room for follow-up polish (keyboard shortcut, auto-pause
when the tab is hidden, "step one tick" while paused) that a contributor
can scope as they like.

Note: as of v0.28, `autoTick` also drives the signaling layer's visible
surface — `COURT` emissions render as `signals N` bubbles, the LISTEN
cue renders `heard N` bubbles, and `mesh.drainAndRoute` routes signals
into siblings' inboxes after every eval. All of these run *inside*
`autoTick`, so pausing the three existing timers stops them too — no
additional pause wiring is needed. Just be aware they exist when
verifying.

## Acceptance criteria

- [ ] A `pause` chip appears in the `#info-right` header next to the
      existing chips (`spawn`, `chatter`, `metrics`, `repl`).
- [ ] Clicking it stops `autoTick`, `autoSpawnCheck`, and the
      `loneChatterTick` interval; the chip toggles its `active` style.
- [ ] Clicking again restores all three timers without losing canvas state.
- [ ] The REPL drawer continues to accept input while paused — typing
      `2 3 + .` still returns `5 ok`.
- [ ] No new dependencies, no build step.

## Where to look

- `web/index.html` — search for `setInterval(autoTick`,
  `setInterval(autoSpawnCheck`, and `loneIntervalId = setInterval` to find
  the three drivers.
- `toggleMute`, `toggleDashboard`, `toggleRepl` are good models for the
  chip wiring.
- The "alife-first" layout-pass section (search the file for
  `LAYOUT PLAN`) explains the surrounding design intent.

## Nice-to-haves (not required)

- Bind the spacebar to toggle pause when the REPL drawer is closed.
- Show a small "paused" indicator overlaid on the canvas.
