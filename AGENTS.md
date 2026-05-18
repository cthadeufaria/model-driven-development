<!-- mdd:begin -->
<!-- mdd:meta {"tool":"mdd","schema":1,"kind":"agents-entrypoint","content_sha256":"eed873bc3a66748ead85dfb1c664d7cc495eca5365b426fa9b28555a9d2d9b6a"} -->
# Agent MDD Entry Point

This repository uses agent-first MDD. Start by reading `.mdd/docs/mdd-workflow.md` and `.mdd/docs/uml-and-ocl-guide.md`.

Codex and other agents should use the project skills in `.codex/skills/`:

- `/mdd-map` — derive current view from existing code into `.mdd/models/current/`.
- `/mdd-generate` — derive objective view from a description into `.mdd/models/objective/`.
- `/mdd-validate` — structural gate over current and objective sides.
- `/mdd-implement` — close the gap between objective and current by writing code.
- `/mdd-review` — strict structural match between current and objective; emits annotated diff PUMLs on mismatch.
- `/mdd-cycle` — orchestration: run the whole loop from one description; owns the cycle boundary, loops to parity, pauses for clarification.
- `/mdd-render` — utility: render PlantUML diagrams to SVG for external visual inspection.
- `/mdd-deploy` — utility: guide an Azure Container Apps deployment via a UML deployment diagram, runbook, and generated Bicep IaC; never executes deploy commands; outside the parity gate.

Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative planning context. Validate IDs, refs, and trace links before implementation; report missing rendering, approval, or acceptance-test readiness as warnings instead of blocking implementation.
<!-- mdd:end -->
