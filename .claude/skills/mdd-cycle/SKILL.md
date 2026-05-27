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
- **Scope provided (realize-slice)** → a slice of an *existing* objective is named (a set of `@id`s — e.g. a PLAN item's `@scope(...)`). **Skip `/mdd-generate`** (the objective is already authored); open a cycle that records `scope: [ids]` in its manifest and loop `/mdd-validate → /mdd-implement → /mdd-map → /mdd-validate → /mdd-review` to **scoped** parity. Only the in-scope `@id`s must reach the current side; objective ids outside the scope that are still absent are **expected**, not a mismatch. Whole-model parity is reached when the last slice closes. `Project::review` reads the open cycle's manifest `scope` automatically (empty = whole-model, the default for the two entries above). This is the entry the greenfield kickoff → Ralph handoff uses.

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
   scope: []              # realize-slice only: objective @ids this cycle realizes; omit/[] = whole-model
   ```

2. **Loop to parity**: `/mdd-validate` → (`/mdd-test` red phase, when `test.layers` is configured) → `/mdd-implement` → **`mdd map-scope`** → `/mdd-map` (scoped) → `/mdd-validate` → `/mdd-review`. On a review mismatch, hand back to `/mdd-implement` and loop. Repeat until `/mdd-review` reports parity matched (ID parity and security parity per `.mdd/config.yml`). See *Test profile and the green gate* for the red phase and the three Close gates.

   **Scoped re-map (cut the loop's token cost).** Before each `/mdd-map`, run `mdd map-scope --cycle <N> --json` and hand the result to `/mdd-map`: it re-derives only the `affected_files` (the current-side diagrams whose backing `.rs`/`src` code `/mdd-implement` just changed, plus the current-side concepts realizing the manifest `scope`), leaving the rest of `current/` byte-identical — so a review-mismatch re-loop costs a handful of files, not the whole 80+-file tree. This is the plan-deterministic / execute-in-skill split (the verb computes the plan; `/mdd-map` executes the re-map), the same shape as `mdd test-plan`. **Honor `full_remap: true`** (a changed source with no `source_link`, surfaced under `scope_escapes`) by re-mapping the whole tree — never silently narrow. `/mdd-validate` still runs whole-tree afterward as the safety net, so a too-narrow scope surfaces as a structural error rather than shipping.
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

## Test profile and the green gate (diagram-driven tests, Cycle B)

When the repo has adopted diagram-driven tests (`.mdd/config.yml` `test.layers` is non-empty), the cycle also runs the linked test suite and gates on green at close. Detection and the plan are deterministic `mdd` verbs; the confirmation and the run are this skill's job — the same plan-deterministic / execute-in-skill split as `/mdd-deploy`.

- **Detect, then confirm (first run / new layer).** If `test.layers` is empty or a needed layer is missing, run `mdd test-detect --json`. It RECOMMENDS a per-layer framework+command from the build files and lists `ambiguities`. Present the recommendation; **surface every ambiguity as a blocking question — never auto-pick a runner**. Write the operator-confirmed profile to `config.test.layers`. No silent default.
- **Red phase, before code (`/mdd-test`).** After `/mdd-validate` and before `/mdd-implement`, hand the cycle's gap set (objective `@id`s absent from current at open) to `/mdd-test`. It realizes each gap's linked test, runs it against the pre-implement code, asserts it FAILS, and records `red` to `.mdd/cycles/<N>/test-evidence.yml`. `/mdd-implement` then turns red→green and records `green`. An empty gap set (pure refactor) writes no evidence.
- **Green gate at close (after parity matches).** Run `mdd test-plan --json` for the ordered steps, execute each step's `command` via Bash, and collect exit codes. Feed them to `Project::evaluate_green_gate` (reads `test.gate`): a still-red test **blocks** close when `test.gate=error`, or is **reported and allows a user-accepted close** when `warn` (the opt-down, like `security.parity_check`).
- **Non-negotiable red→green gate at close.** Also call `Project::evaluate_red_green_gate(evidence, gap_ids)`: the cycle closes only when every gap `@id` shows fail-then-pass. This has **no config off-switch** (distinct from `test.gate`, which governs only the green side). A gap recorded red-as-pass is a blocking question, never accepted.
- **Three gates at Close.** Parity (`Project::review()`) AND the non-negotiable red→green evidence AND the green gate (per `test.gate`) must all pass to close.
- **Inert by default.** With no configured layers all of this does nothing, so a repo that has not adopted diagram-driven tests closes exactly as before.

## Readiness

Rendered SVGs, approvals, and acceptance-test gaps are readiness warnings — report and continue unless the user asks to pause. Structural validation errors block the loop until fixed.
