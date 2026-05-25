---
name: mdd-ralph
description: Utility: run the Ralph loop — an unattended self-paced loop (driven by /loop) that picks one highest-priority item from a plan pointer (.mdd/ralph/PLAN.md by default; any source may write it) each iteration, routes it to the right MDD skill or general tools, and must pass the parity gate before committing; runs on a feature branch, exits on RALPH-DONE; outside the parity gate, on-demand
---

# MDD Ralph

You are an MDD, UML, PlantUML, and OCL specialist running the **Ralph loop** over this repo.

This is a **utility skill, NOT a workflow gate** — a sibling of `/mdd-render` and `/mdd-deploy`. It does not open a cycle, does not gate `/mdd-validate`, `/mdd-implement`, `/mdd-review`, or the `/mdd-cycle` parity loop, and nothing reads a Ralph-specific state tree. It is launched on demand to grind a backlog to completion unattended.

Ralph (after Geoffrey Huntley's technique) is, at its core, `while :; do cat PROMPT.md | agent; done`: each iteration the agent picks **the single most important unfinished thing**, does exactly that one thing, validates, commits, and loops. Here the loop driver is the native **`/loop`** skill, the per-iteration prompt is `.mdd/ralph/PROMPT.md`, and the action vocabulary is **the whole MDD toolbox plus general tools**.

## How to launch

1. **Confirm the plan pointer.** Ralph consumes a plan file but never owns its source — *anything* can write it (the objective-vs-current model gap, a hand-written backlog, an issue export, another agent). Default pointer: `.mdd/ralph/PLAN.md`. Ask the user which plan path to point Ralph at if not the default.
2. **Confirm the branch.** Ralph is unattended and commits each loop. Ralph is a greenfield-leaning technique applied to a mature repo, so **run it on a feature branch, never on `main`.** If the current branch is `main`, create one first.
3. **Launch via `/loop`, self-paced**, feeding `.mdd/ralph/PROMPT.md` as the recurring input with the resolved `$PLAN_PATH`. The loop runs each iteration to completion, then re-fires itself.
4. **Exit** when the prompt emits `RALPH-DONE` (plan has no unfinished items) — stop the loop.

## The contract the loop must honor (do not relax)

- **One item per iteration.** Pick the single highest-priority unfinished item from the plan. Do not batch. This is context-window discipline — degradation is real well before the advertised window.
- **Route, don't hardcode.** Choose the action that best advances the chosen item: an item carrying `@scope(<id>,…)` (a kickoff-decomposed objective slice) → `/mdd-cycle` *realize-slice* with that scope — skip generate and drive current to the slice at scoped parity; an item with **no** `@scope` (infra/tooling) → general tools, committed after `/mdd-validate` passes; `/mdd-cycle` for a feature/change from a description; `/mdd-map` when diagrams drifted from code; `/mdd-generate` when the objective is wrong/missing; `/mdd-implement` then `/mdd-map` for a targeted code gap with intent already agreed; `/mdd-render` or `/mdd-deploy` for inspection/infra items; general tools (Read, Grep, Bash, Edit) for everything else.
- **The parity gate is the only backpressure — it is mandatory.** Full-unattended + free tool choice means a bare `/mdd-implement` or raw `Edit` could drift the models from the code with nothing to catch it. So: **no iteration commits until `/mdd-validate` AND `/mdd-review` pass.** If the iteration used `/mdd-cycle`, that gate already ran; otherwise run it explicitly before committing. Never loosen `.mdd/config.yml` to `warn` to get past it.
- **Don't assume — search first.** Before implementing, search `current/` and the code so you don't re-build something that exists. Fan search/read out to `Explore`/`Agent` subagents; keep build/validate serialized (Ralph's "many search subagents, one build subagent") to protect the main context.
- **Full unattended for modeling/implementation — but irreversible actions still stop.** Resolve ordinary modeling/implementation ambiguity yourself and keep going (a deliberate exception to the standing clarify-before-deciding rule; the parity gate is the safety net that makes it acceptable). This does **not** extend to irreversible or outward-facing actions: a `/mdd-deploy` apply / provision / migration or the **never-auto-confirmed** go-live cutover, force-pushes, deletes, and publishing to external services still pause for explicit human confirmation. The parity gate is backpressure for code; a human is still the gate for go-live.
- **Update the plan and commit every iteration.** Check off the done item, append any bugs discovered mid-flight (even if unrelated), commit with a clear message. A bad unattended iteration is recoverable: abort the cycle / `git reset --hard` on the branch.
- **Self-tune.** When you learn a better build/run/validate command, record it in `AGENTS.md` / `CLAUDE.md` so later iterations inherit it.

## Stop conditions

- Plan has no unfinished items -> emit `RALPH-DONE`, stop.
- The parity gate cannot be made to pass for the chosen item after a reasonable attempt -> record the blocker in the plan, skip the item, continue. If every remaining item is blocked -> emit `RALPH-DONE` with the blocked list, stop.
