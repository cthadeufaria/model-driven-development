<!-- mdd:begin -->
<!-- mdd:meta {"tool":"mdd","schema":1,"kind":"agents-entrypoint","content_sha256":"79493682c18bc67eb9059e9385be914dd6ac5f2c142d88d0ca8d5246261edd3d"} -->
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
- `/mdd-deploy` — utility: plan then EXECUTE an Azure Container Apps deployment via a UML deployment diagram, runbook, and generated Bicep or Terraform IaC (operator-confirmed dialect + purpose); runs the runbook to live traffic, pausing only at irreversible steps + a never-auto-confirmed go-live gate and halting on first error; outside the parity gate.
- `/mdd-ralph` — utility: run the Ralph loop — an unattended, self-paced loop (driven by `/loop`) that takes one highest-priority item per iteration from a plan pointer (`.mdd/ralph/PLAN.md` by default; any source may write it), routes it to the right MDD skill or general tools, and must pass the parity gate before committing; runs on a feature branch, exits on `RALPH-DONE`; outside the parity gate.

## Session start: diagrams-first

At the start of a session, run `mdd context`. It prints a compact whole-map table of contents plus a freshness verdict. Codex and other agents cannot auto-fire a session hook, so **run `mdd context` yourself at the start of each session**. Then read in that order:

- **FRESH** → trust the diagrams: read the relevant concept diagrams under `.mdd/models/current/`, follow `.mdd/trace.yml` `source_links` to the code, then act.
- **STALE** → the code drifted from the diagrams: run `/mdd-map` on the drifted area FIRST to re-derive `current/`, then read the refreshed diagrams.

Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative planning context. Validate IDs, refs, and trace links before implementation; report missing rendering, approval, or acceptance-test readiness as warnings instead of blocking implementation.
<!-- mdd:end -->
