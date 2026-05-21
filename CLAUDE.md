<!-- mdd:begin -->
<!-- mdd:meta {"tool":"mdd","schema":1,"kind":"claude-entrypoint","content_sha256":"80a669791c32cb3530bb670fc742a5838e349a25cbb45ed845ba1ed6531b2c08"} -->
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
- `/mdd-deploy` — guide an Azure Container Apps deployment via a UML deployment diagram, runbook, and generated Bicep or Terraform IaC (operator-confirmed dialect, one per run); never executes deploy commands; outside the parity gate.

## Session start: diagrams-first

At the start of a session, run `mdd context` — a Claude Code SessionStart hook (wired by `mdd init` into `.claude/settings.json`) runs it for you and injects the result. It prints a compact whole-map table of contents plus a freshness verdict. Read in that order:

- **FRESH** → trust the diagrams: read the relevant concept diagrams under `.mdd/models/current/`, follow `.mdd/trace.yml` `source_links` to the code, then act.
- **STALE** → the code drifted from the diagrams: run `/mdd-map` on the drifted area FIRST to re-derive `current/`, then read the refreshed diagrams.

Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative planning context. Validate IDs, refs, and trace links before implementation; report missing rendering, approval, or acceptance-test readiness as warnings instead of blocking implementation.
<!-- mdd:end -->
