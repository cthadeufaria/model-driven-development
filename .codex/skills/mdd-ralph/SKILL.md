---
name: mdd-ralph
description: Utility: launch the Ralph loop — a one-shot launcher that starts the `ralph-loop` plugin with `.mdd/ralph/PROMPT.md` as the per-iteration prompt and `.mdd/ralph/PLAN.md` as the backlog, picking one item per iteration, verifying via the parity gate before each commit; runs on a feature branch, exits on `<promise>RALPH-DONE</promise>`; outside the parity gate, on-demand
---

# MDD Ralph

Invoking this skill **launches the Ralph loop** via the `ralph-loop` plugin. It is exactly equivalent to running:

```
/ralph-loop "$(cat .mdd/ralph/PROMPT.md)" --completion-promise 'RALPH-DONE' --max-iterations 40
```

`.mdd/ralph/PROMPT.md` is the per-iteration prompt (re-fed unchanged each iteration by the plugin's Stop hook); `.mdd/ralph/PLAN.md` is the backlog Ralph grinds down. The skill just removes the need to type that whole command.

## Launch — do this when invoked

1. **Branch check.** Ralph commits every iteration — **never on `main`**. If on `main`, create/switch to a feature branch first.
2. **Plan check.** Confirm `.mdd/ralph/PLAN.md` has unfinished `- [ ]` items. If it has none, tell the user there's nothing to do and stop.
3. **Activate the loop.** Run this in Bash (discovers the installed plugin script, then starts the loop with the prompt + completion promise + iteration cap):
   ```bash
   SCRIPT=$(ls ~/.claude/plugins/cache/*/ralph-loop/*/scripts/setup-ralph-loop.sh 2>/dev/null | head -1)
   [ -z "$SCRIPT" ] && SCRIPT=$(ls ~/.claude/plugins/marketplaces/*/plugins/ralph-loop/scripts/setup-ralph-loop.sh 2>/dev/null | head -1)
   [ -z "$SCRIPT" ] && { echo "ralph-loop plugin not found — install it via /plugin, then re-run /mdd-ralph"; exit 1; }
   bash "$SCRIPT" "$(cat .mdd/ralph/PROMPT.md)" --completion-promise 'RALPH-DONE' --max-iterations 40
   ```
   If the script isn't found, stop and tell the user to install the `ralph-loop` plugin (`/plugin`).
4. **Begin iteration 1** by following the now-active prompt (read `.mdd/ralph/PLAN.md`, do the single topmost item, verify, commit). The Stop hook then re-feeds the prompt each iteration until you emit `<promise>RALPH-DONE</promise>` or the 40-iteration cap is hit.

## Stop / monitor

- Monitor: `head -10 .claude/ralph-loop.local.md` (shows the current `iteration:`).
- Stop early: `/cancel-ralph` (or `rm .claude/ralph-loop.local.md`).
- To change the per-iteration behaviour or the cap, edit `.mdd/ralph/PROMPT.md` (or the flags above) and re-run `/mdd-ralph` — the plugin snapshots the prompt at launch, so edits need a relaunch.

## Contract (lives in PROMPT.md, do not relax)

- **One item per iteration** — context discipline; no batching.
- **The parity gate is mandatory backpressure** — `mdd validate` + `mdd review` must pass before any commit. Never loosen `.mdd/config.yml`.
- **Full-unattended for modeling/implementation, but stop for irreversible/outward-facing actions** — force-pushes, deletes, deploys, publishing externally need explicit human confirmation.
- **Only emit `<promise>RALPH-DONE</promise>` when the plan is genuinely exhausted or fully blocked** — never to escape the loop.
