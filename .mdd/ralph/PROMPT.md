# Ralph loop — per-iteration prompt (for `/ralph-loop`)

Run with: `/ralph-loop "$(cat .mdd/ralph/PROMPT.md)" --completion-promise 'RALPH-DONE' --max-iterations 40`
(Plan defaults to `.mdd/ralph/PLAN.md`; point at a different plan by editing the path below.)

This prompt is re-fed unchanged every iteration — state lives in the files and git
history, not here. So re-orient from the plan each time. Do **exactly one** item, then stop.

## This iteration

1. **Read the plan** at `.mdd/ralph/PLAN.md`. If it has no unfinished `- [ ]` items,
   output `<promise>RALPH-DONE</promise>` as your final message and do nothing else.
2. **Pick the single topmost unfinished `- [ ]` item.** One item only — no batching.
3. **Search before assuming.** Confirm it isn't already done — check the code and
   `.mdd/models/current/`. Fan reads out to `Explore`/`Agent` subagents; keep build/validate serialized.
4. **Do that one item end-to-end** with general tools (Read/Grep/Edit/Bash).
5. **Verify — mandatory before commit.** Run the build and tests until green, then run the
   deterministic parity gate via the CLI: `mdd validate` and `mdd review`. Do not commit on a
   failure; do not loosen `.mdd/config.yml`. (These are CLI verbs — you do not need the `/mdd-*` skills.)
6. **Update the plan and commit.** Check the item off (`- [x]`) in `.mdd/ralph/PLAN.md`, append any
   new bugs/work found as new `- [ ]` items, and commit with a clear message. Feature branch only — never `main`.
7. If the item can't be finished after a reasonable attempt, note the blocker under it, leave it
   unchecked, and stop (next iteration moves on). If every remaining item is blocked, output `<promise>RALPH-DONE</promise>`.

## Rules

- One item per iteration — context discipline.
- Resolve ordinary implementation ambiguity yourself; keep going. The parity gate is the safety net.
- **Stop for irreversible / outward-facing actions** — force-pushes, deletes, deploys, publishing externally — these still need explicit human confirmation.
- Only output `<promise>RALPH-DONE</promise>` when the plan is genuinely exhausted (or fully blocked). Do not emit it to escape the loop.
