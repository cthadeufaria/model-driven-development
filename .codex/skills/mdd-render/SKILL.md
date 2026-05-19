---
name: mdd-render
description: Utility: render PlantUML diagrams to SVG for external visual inspection
---

# MDD Render

You are an MDD, UML, PlantUML, and OCL specialist for diagram rendering.

This is a **utility skill, not a workflow gate**, and a **thin wrapper** over the `mdd render` command. The mechanics — enumerating every renderable tree, synthesizing OCL constraint diagrams, the PlantUML/Graphviz subprocess, jar resolution, and the deterministic source→`.mdd/rendered/` path mirror — all live in compiled code (the `mdd-render` engine, driven by `mdd render`, whose tree set is the single `mdd-core` `Project` enumeration). You add only **judgment**: interpreting a fuzzy subset request, triaging diagnostics, and suggesting fixes. Do **not** hand-run `java -jar plantuml.jar` or re-implement the tree list here.

## Workflow

1. **Fuzzy subset intake.** Translate what the user asked for into `mdd render` arguments:
   - whole system / "render everything" / a cycle just closed → `mdd render` (no args = full tree parity: models, cycle diffs, OCL, whole-map, deploy, review-diff).
   - one tree, e.g. "just the deploy diagrams", "the OCL diagrams", "the whole-map" → `mdd render --only deploy` (selectors: `models`, `cycle-diffs`, `ocl`, `map`, `deploy`, `review`; comma-separate for several).
   - specific files/dirs, e.g. "just current use-cases" → `mdd render .mdd/models/current/use-cases`.
2. **Run it.** Invoke the resolved `mdd render …`. It writes each source to its deterministic `.mdd/rendered/` mirror and prints `rendered <path>` lines plus `diagnostic <path>: <message>` lines; it exits non-zero if any diagnostic occurred.
3. **Diagnostic triage + fix suggestions.** For each `diagnostic` line, explain the likely cause and the concrete fix, e.g.:
   - `Cannot find Graphviz` / `Dot executable does not exist` → install Graphviz (`brew install graphviz`), or set `GRAPHVIZ_DOT=/path/to/dot`.
   - `PlantUML is not available` → install the bundled jar + Java, set `MDD_PLANTUML_JAR=/path/to/plantuml.jar`, or put `plantuml` on PATH.
   - `Syntax Error` / `No diagram found` → point at the offending source file so it can be fixed at the model.
4. **Report** the rendered list and any diagnostics with their suggested fixes. The user reviews the SVGs externally.

## Cross-skill contract (do not move)

`/mdd-cycle`'s close step, `/mdd-review`, and `/mdd-deploy` hand off to `/mdd-render` by name. That name and this skill stay; only the mechanics moved into `mdd render`. Those callers may also invoke `mdd render` directly — same engine, same single tree set.

Rendering is not a gate. Validation, implementation, and review do not depend on a render pass.
