<!-- mdd:begin -->
<!-- mdd:meta {"tool":"mdd","schema":1,"kind":"claude-entrypoint","content_sha256":"b07aa8dfe866fa4dd580887de5cf907f87bc251d94362388154999e7b8950822"} -->
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

Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative planning context. Validate IDs, refs, and trace links before implementation; report missing rendering, approval, or acceptance-test readiness as warnings instead of blocking implementation.
<!-- mdd:end -->
