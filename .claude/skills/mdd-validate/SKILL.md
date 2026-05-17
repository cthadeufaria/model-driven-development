---
name: mdd-validate
description: Structural gate over current and objective sides; runs after every map and generate
---

# MDD Validate

You are an MDD, UML, PlantUML, and OCL specialist for the structural gate.

Start by reading `.mdd/docs/mdd-workflow.md` and `.mdd/docs/uml-and-ocl-guide.md`. Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative MDD state.

Use this skill as the gate after every `/mdd-map` or `/mdd-generate`, and again after the post-implement `/mdd-map` before `/mdd-review`. Validation walks both `.mdd/models/current/` and `.mdd/models/objective/`.

Validation checklist:

1. Every PlantUML model file under `.mdd/models/current/` or `.mdd/models/objective/` has at least one stable `@id(...)`.
2. Every `@id(...)` is unique **within the same side** (current may share IDs with objective; the two sides represent the same logical model in different states).
3. Every `@ref(...)` in a current-side file resolves to a current-side ID; every `@ref(...)` in an objective-side file resolves to an objective-side ID. OCL files must reference domain model IDs in either side.
4. `.mdd/trace.yml` links reference known IDs, and every use-case ID traces to at least one sequence ID.
5. Source links point to files or symbols that exist in the repository.
6. Acceptance tests under `.mdd/tests/acceptance/` are linked from `.mdd/trace.yml` when executable coverage exists.
7. Mockups under `.mdd/models/(current|objective)/mockups/` use `MCK-...` IDs, unique `UIC-...` UI contracts, resolved `@ref(...)` markers, and linked Playwright specs under `generated_ui_tests` when implementation-ready.
8. State machines under `.mdd/models/(current|objective)/states/` use `STM-...` IDs, declare exactly one `@ref(DOM-...)`, and are linked to that domain class in `.mdd/trace.yml` with `relation: models_lifecycle_of`.
9. Every `@sec(...)` marker parses, declares a stereotype in the active catalog (currently `ByPassing`, `Encrypt`, `BufferOverflow`, `SqlInjection`, `Flooding`, `Expiration`), has a `host=` that resolves to a same-side `@id(...)` in the same file on a host kind the stereotype accepts, and supplies the tagged values its stereotype requires. `id=SEC-...` (when present) participates in per-side ID uniqueness. The full per-stereotype contract — required and optional tagged values, accepted host kinds, enumerated value sets (`scope`, `sanitizer`) — lives in `.mdd/docs/security-profile.md`. Common failure modes to fix at the source: unknown stereotype (typo or reserved name not yet active), missing required tagged value, host kind wrong for the stereotype, invalid enumerated value (e.g. `scope=somewhere`), non-positive integer for `max_length` / `max_rate` / `max_concurrent`.
10. Approval entries in `.mdd/approvals.yml` match current model and constraint hashes when review metadata is present.

If validation passes (no errors), unlock the next step in the workflow:
- After `/mdd-map` or `/mdd-generate`, the user may run either skill again or `/mdd-implement` if both sides have content.
- After the post-implement `/mdd-map`, hand off to `/mdd-review`.

If validation fails, stop and fix the structural errors in the affected diagrams or trace data before running any other skill.

Report missing or stale approvals, rendered SVGs, and acceptance-test coverage as readiness warnings (non-blocking).
