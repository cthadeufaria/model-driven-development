---
name: mdd-render
description: Utility: render PlantUML diagrams to SVG for external visual inspection
---

# MDD Render

You are an MDD, UML, PlantUML, and OCL specialist for diagram rendering.

This is a **utility skill, not a workflow gate**. Use it whenever the user wants to open `.mdd/models/**` PlantUML files as SVGs in an external editor for visual inspection.

## Workflow

1. Render each PlantUML file under `.mdd/models/` to the matching `.mdd/rendered/models/.../*.svg` path. If the user specifies a subset (e.g. only `current/use-cases`), render that subset.
2. Prefer the repository or packaged PlantUML jar when available (e.g. `third_party/plantuml/plantuml.jar`). Otherwise use `plantuml` on PATH. Java jar rendering: `java -jar path/to/plantuml.jar -tsvg -pipe < <input.puml> > <output.svg>`.
3. Ensure Java is available for jar rendering and Graphviz `dot` is available for graph-based UML diagrams.
4. After rendering, inspect each SVG for PlantUML diagnostic text (`Dot executable does not exist`, `Cannot find Graphviz`, `Syntax Error`, `Error`, `No diagram found`) and report any findings.
5. Also render any `.diff.puml` files under `.mdd/rendered/review/` produced by `/mdd-review` so the user can inspect the diff diagrams.
6. Also rasterize every `.mdd/cycles/<N>/<rel>.diff.puml` to its deterministic mirror `.mdd/rendered/cycles/<N>/<rel>.diff.svg` (`.mdd/cycles/` → `.mdd/rendered/cycles/`, `.diff.puml` → `.diff.svg`) so the viewer's Diff mode can paint the superposed diagram for the selected file.
7. Also rasterize every `.mdd/map/<kind>/<name>.puml` — the whole-map baseline that `/mdd-cycle`'s close step accumulates — to its deterministic mirror `.mdd/rendered/map/<kind>/<name>.svg` (`.mdd/map/` → `.mdd/rendered/map/`) so the accumulated whole-system picture can be inspected externally. An absent `.mdd/map/` tree is not an error.
8. Also rasterize every `.mdd/deploy/**/*.puml` — the deployment diagrams produced by the `/mdd-deploy` utility skill — to its deterministic mirror `.mdd/rendered/deploy/**/*.svg` (`.mdd/deploy/` → `.mdd/rendered/deploy/`) so the deployment diagram is inspectable like every other diagram. Additive and non-gating; an absent `.mdd/deploy/` tree is not an error.
9. Also synthesize a PlantUML constraints diagram from every `.mdd/constraints/*.ocl` and rasterize it to `.mdd/rendered/constraints/<name>.svg` (via `mdd_render::render_ocl_diagrams`) so the viewer's OCL Diagram sub-mode can paint it.
10. Report the list of rendered files and any diagnostic failures. The user reviews them externally.

Rendering is not a gate. Validation, implementation, and review do not depend on a render pass.
