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
3. **Close**: copy `.mdd/models/current/` to `.mdd/cycles/<N>/after/`. For every diagram whose element set changed between `before/` and `after/`, write an annotated `<diagram>.diff.puml` under `.mdd/cycles/<N>/` (shared elements once, additions `<<added>>` green, removals `<<removed>>` red), then rasterize each to its deterministic mirror `.mdd/rendered/cycles/<N>/<rel>.diff.svg` (via `/mdd-render` or `mdd_render::render_cycle_diffs`) so the viewer's Diff mode can paint it, and run `mdd_render::render_ocl_diagrams` so the viewer's OCL Diagram sub-mode can paint constraint files. **Then accumulate the whole-map baseline** (see *Whole-map baseline* below). Update the manifest: `status: closed`, add `closed_at`, and set `touched_files` to the model files this cycle changed.
4. **Abort**: if the user cancels, set `status: aborted` and leave snapshots as-is.

## Authoring rule for descriptions

Every significant `@id(...)` authored by `/mdd-generate` or `/mdd-map` during the cycle must carry a one-line `@desc(<ID>, "what this element is")` marker in the same file, so the viewer's MODEL CONTEXT card can describe it on selection.

## Whole-map baseline

After the cycle's `<diagram>.diff.puml` files are written and before the manifest is closed, fold this cycle's diff into the persisted **whole-map** under `.mdd/map/` so it grows into a complete per-concept picture of the system, cycle by cycle. The whole-map is **not** re-derived from code and is **not** a `/mdd-map` mode — it is maintained one cheap `CycleDiff` application per cycle:

1. For every concept file `<kind>/<name>` present in this cycle's `after/`, take its `CycleDiff` (the same `@id` add/remove sets used for `<diagram>.diff.puml`):
   - If `.mdd/map/<kind>/<name>.puml` does not exist, create it as a verbatim copy of the `after/` file, then add — right after the `@startuml`/`@startsalt` line — a comment block with one `' @cycle(<ID>, <N>)` line per `@id(...)` in the file.
   - Otherwise, in the existing whole-map file: insert each **added** `@id` and its element (copied from `after/`) with a `' @cycle(<ID>, <N>)` provenance line; delete each **removed** `@id` and its element; leave **unchanged** `@id`s and their earlier `' @cycle(...)` provenance untouched. Net cancellation is automatic — a later remove physically deletes whatever an earlier cycle added, so an added-then-removed element ends in **neither** (no `<<removed>>` ghost, unlike a single cycle's `.diff.puml`).
2. For a concept file in `before/` but absent from `after/` (the whole file was deleted), remove `.mdd/map/<kind>/<name>.puml`.
3. Rewrite `.mdd/map/manifest.yml`: `version: 1`, `last_cycle: <N>`, `generated_at: "<ISO-8601>"`, and `files:` listing every `<kind>/<name>.puml` written.
4. Copy the whole `.mdd/map/` tree into `.mdd/cycles/<N>/whole/` so the system picture *as of cycle N* is recoverable without replay.
5. Hand the new `.mdd/map/**.puml` to `/mdd-render` to rasterize to `.mdd/rendered/map/**.svg`.

The whole-map is an **inspection artifact, outside the parity gate**: `/mdd-validate`, `/mdd-review`, and this skill's own parity loop never read or gate on `.mdd/map/`. The `OCL-MAP-*` constraints in `.mdd/constraints/whole-map.ocl` describe its invariants but are not parity checks. Greenfield (no closed cycle) means no `.mdd/map/` tree at all.

## Readiness

Rendered SVGs, approvals, and acceptance-test gaps are readiness warnings — report and continue unless the user asks to pause. Structural validation errors block the loop until fixed.
