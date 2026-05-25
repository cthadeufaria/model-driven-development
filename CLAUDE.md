<!-- mdd:begin -->
<!-- mdd:meta {"tool":"mdd","schema":1,"kind":"claude-entrypoint","content_sha256":"2559025839e0b0634ba9019dab5cc51ec7d60e80eee536a67689a391d114b63d"} -->
# Claude Code MDD Entry Point

This repository uses agent-first MDD. Start by reading `.mdd/docs/mdd-workflow.md` and `.mdd/docs/uml-and-ocl-guide.md`.

Workflow skills in `.claude/skills/`:

- `/mdd-map` — derive current view from existing code into `.mdd/models/current/`.
- `/mdd-generate` — derive objective view from a description into `.mdd/models/objective/`.
- `/mdd-validate` — structural gate over current and objective sides.
- `/mdd-implement` — close the gap between objective and current by writing code.
- `/mdd-review` — strict structural match between current and objective; emits annotated diff PUMLs on mismatch.

Orchestration skill (runs the whole loop from one description):

- `/mdd-cycle` — selects the entry point, owns the cycle boundary under `.mdd/cycles/`, loops to parity, and pauses for clarification.

Utility skills (on demand, not a workflow gate):

- `/mdd-render` — render PlantUML diagrams to SVG for external visual inspection.
- `/mdd-deploy` — plan then EXECUTE an Azure Container Apps deployment: generates a UML deployment diagram, runbook, and Bicep or Terraform IaC (operator-confirmed dialect + purpose), then runs the runbook to live traffic — managing auth, dry-running, applying, provisioning, migrating, and routing traffic, pausing only at irreversible steps and a never-auto-confirmed go-live gate, halting on first error; outside the parity gate.
- `/mdd-ralph` — run the Ralph loop: an unattended, self-paced loop (driven by `/loop`) that takes one highest-priority item per iteration from a plan pointer (`.mdd/ralph/PLAN.md` by default; any source may write it), routes it to the right MDD skill or general tools, and must pass the parity gate before committing. Runs on a feature branch, never `main`; exits on `RALPH-DONE`; outside the parity gate.

## Session start: diagrams-first

At the start of a session, run `mdd context` — a Claude Code SessionStart hook (wired by `mdd init` into `.claude/settings.json`) runs it for you and injects the result. It prints a compact whole-map table of contents plus a freshness verdict. Read in that order:

- **FRESH** → trust the diagrams: read the relevant concept diagrams under `.mdd/models/current/`, follow `.mdd/trace.yml` `source_links` to the code, then act.
- **STALE** → the code drifted from the diagrams: run `/mdd-map` on the drifted area FIRST to re-derive `current/`, then read the refreshed diagrams.

Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative planning context. Validate IDs, refs, and trace links before implementation; report missing rendering, approval, or acceptance-test readiness as warnings instead of blocking implementation.
<!-- mdd:end -->
