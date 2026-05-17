---
name: mdd-cycle
description: Run a full MDD cycle from one description; owns the cycle boundary, loops to parity, pauses for clarification
---

# MDD Cycle

You are an MDD, UML, PlantUML, and OCL specialist for orchestrating one complete MDD cycle end to end.

Start by reading `.mdd/docs/mdd-workflow.md` and `.mdd/docs/uml-and-ocl-guide.md`. Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative MDD state.

Use this skill to run a whole cycle from a single feature/bug-fix/change description. It selects the entry point, **owns the cycle boundary**, and loops the productive skills to parity, pausing only to ask for clarification.

## Clarification is mandatory

Whenever a modeling or implementation decision is genuinely ambiguous, **stop and ask the user** before proceeding. Never guess an ambiguous decision. Resume only after the user answers. This rule overrides autonomy: a paused, correct cycle beats a fast, wrong one.

## Entry-point selection

- **Description provided** → entry is `/mdd-generate` (derive the objective from the description), then run the full loop below.
- **No description** → behave **exactly as `/mdd-map` with no comments** does: refresh the current-side use-case view only, then stop. Do **not** open a cycle, run the loop, or write snapshots.

## Cycle boundary (this skill owns it)

Standalone `/mdd-map` and `/mdd-generate` never open or close a cycle — only this skill does.

1. **Open**: pick the next zero-padded number `N` (4 digits) under `.mdd/cycles/`. Create `.mdd/cycles/<N>/`, copy the entire `.mdd/models/current/` tree to `.mdd/cycles/<N>/before/` (an empty tree is fine), and write `.mdd/cycles/<N>/manifest.yml`:

   ```yaml
   number: <N>
   slug: <kebab-slug-of-description>
   entry: generate        # or: map
   description: "<the description>"
   status: open
   opened_at: "<unix-seconds-or-ISO>"
   touched_files: []
   ```

2. **Loop to parity**: `/mdd-validate` → `/mdd-implement` → `/mdd-map` → `/mdd-validate` → `/mdd-review`. On a review mismatch, hand back to `/mdd-implement` and loop. Repeat until `/mdd-review` reports parity matched (ID parity and security parity per `.mdd/config.yml`).
3. **Close**: copy `.mdd/models/current/` to `.mdd/cycles/<N>/after/`. For every diagram whose element set changed between `before/` and `after/`, write an annotated `<diagram>.diff.puml` under `.mdd/cycles/<N>/` (shared elements once, additions `<<added>>` green, removals `<<removed>>` red). Update the manifest: `status: closed`, add `closed_at`, and set `touched_files` to the model files this cycle changed.
4. **Abort**: if the user cancels, set `status: aborted` and leave snapshots as-is.

## Authoring rule for descriptions

Every significant `@id(...)` authored by `/mdd-generate` or `/mdd-map` during the cycle must carry a one-line `@desc(<ID>, "what this element is")` marker in the same file, so the viewer's MODEL CONTEXT card can describe it on selection.

## Readiness

Rendered SVGs, approvals, and acceptance-test gaps are readiness warnings — report and continue unless the user asks to pause. Structural validation errors block the loop until fixed.
