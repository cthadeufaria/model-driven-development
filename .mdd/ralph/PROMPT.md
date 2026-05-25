# Ralph loop — per-iteration prompt

You are running one iteration of the **Ralph loop** for this repo. The plan pointer for this run is `$PLAN_PATH` (default: `.mdd/ralph/PLAN.md`). Read `.claude/skills/mdd-ralph/SKILL.md` for the full contract; this file is the per-loop instruction.

Do **exactly one** iteration, then return so the loop can re-fire.

## This iteration

1. **Read the plan** at `$PLAN_PATH` and the objective models under `.mdd/models/objective/`. If the plan has no unfinished items, emit `RALPH-DONE` and stop the loop.
2. **Pick the single highest-priority unfinished item.** One item. No batching.
3. **Search before assuming.** Verify it isn't already implemented — search `current/` and the code, fanning reads out to `Explore`/`Agent` subagents. Keep build/validate serialized.
4. **Route to the right action** for that one item:
   - feature/change from a description -> `/mdd-cycle "<item>"`
   - diagrams drifted from code -> `/mdd-map`, then re-plan
   - objective spec wrong/missing -> `/mdd-generate`
   - targeted code gap, intent already agreed -> `/mdd-implement` then `/mdd-map`
   - inspection / infra -> `/mdd-render` or `/mdd-deploy`
   - anything else -> general tools (Read, Grep, Bash, Edit)
5. **Pass the parity gate — mandatory before commit.** If you did not run `/mdd-cycle` (which gates internally), run `/mdd-validate` and `/mdd-review` now and make them pass. Do not commit on a gate failure; do not loosen `.mdd/config.yml`.
6. **Update the plan and commit.** Check off the completed item in `$PLAN_PATH`, append any bugs found mid-flight, and commit with a clear message. If the gate can't be made to pass after a reasonable attempt, record the blocker, skip the item, and continue. If every remaining item is blocked, emit `RALPH-DONE` with the blocked list and stop.
7. **Self-tune** if you learned a better build/run/validate command: record it in `AGENTS.md` / `CLAUDE.md`.

## Rules (do not break)

- Full unattended for **modeling and implementation** decisions: resolve ordinary ambiguity yourself, never pause for it. The parity gate is the safety net.
- **BUT still stop for irreversible / outward-facing actions.** Full-unattended never overrides these — anything hard to undo or that publishes externally needs explicit human confirmation: a `/mdd-deploy` apply / provision / migration / go-live cutover (the go-live gate is **never** auto-confirmed), force-pushes, deletes, or sending to an external service. Pause and ask for these even though everything else runs unattended.
- Never commit to `main`; this loop runs on a feature branch.
- One item per iteration — context discipline.
