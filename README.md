# mdd

`mdd` bootstraps an agent-first model-driven development workspace. The public CLI is intentionally small: it creates or removes durable project-local MDD instructions, then Claude Code, Codex, or another agent performs the lifecycle from those repo-local skills.

## Command

```bash
cargo run -p mdd-cli -- init
cargo run -p mdd-cli -- clean
```

## Install

`mdd` is installed via Cargo. Install the system render dependencies (Graphviz `dot` and PlantUML), then build and install the CLI from this repository:

```bash
brew install graphviz plantuml
cargo install --path crates/mdd-cli --force
```

The `--force` flag overwrites any existing `mdd` binary in `~/.cargo/bin`, so the same command works for a first install and for replacing the current installation with a fresh build from the working tree.

Verify the new binary is picked up:

```bash
which mdd
mdd --version
```

After installation, the binary command is:

```bash
mdd init
```

`mdd init` creates:

- `.mdd/models/{current,objective}/{use-cases,sequences,domain,components,mockups,states}/`
- `.mdd/constraints/`
- `.mdd/rendered/`
- `.mdd/tests/acceptance/`
- `.mdd/tests/ui/`
- `.mdd/trace.yml`
- `.mdd/approvals.yml`
- `.mdd/config.yml`
- `.mdd/docs/mdd-workflow.md`
- `.mdd/docs/uml-and-ocl-guide.md`
- `.claude/skills/<skill-name>/SKILL.md`
- `.codex/skills/<skill-name>/SKILL.md`
- `CLAUDE.md`
- `AGENTS.md`

When a generated file path already exists, `mdd init` prompts before writing. Choose overwrite to replace that file with the generated template, or skip to leave the existing file unchanged.

`mdd clean` removes the generated MDD artifacts. It removes `.mdd/` recursively and removes generated MDD skill files under `.claude/skills/<skill-name>/SKILL.md` and `.codex/skills/<skill-name>/SKILL.md`. It leaves `CLAUDE.md`, `AGENTS.md`, `.claude/`, `.codex/`, `.claude/skills/`, and `.codex/skills/` in place. Use `mdd clean --force` to remove modified generated MDD skill files.

## Agent Skills

The initialized workspace installs matching Claude Code and Codex project skills.

**Workflow skills (gated cycle):**

- `mdd-map`: derive the **current** view from existing code into `.mdd/models/current/`.
- `mdd-generate`: derive the **objective** view from a description into `.mdd/models/objective/`. Absorbs UI mockup authoring.
- `mdd-validate`: structural gate over both sides; per-side ID uniqueness and ref resolution.
- `mdd-implement`: close the gap between objective and current by writing code (no diagram writes).
- `mdd-review`: strict structural match between current and objective; emits annotated `.diff.puml` files under `.mdd/rendered/review/` on mismatch.

**Utility skill (on demand, not a workflow gate):**

- `mdd-render`: render PlantUML diagrams to SVG for external visual inspection.

Agents should start with `.mdd/docs/mdd-workflow.md`, use the installed skills, and treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative.

## Model Rules

Models are text-first PlantUML files. Constraints are OCL files. Model elements use stable `@id(...)` markers, and dependencies use `@ref(...)` markers. Refs resolve within the same side (current or objective). UI mockups live under `.mdd/models/<side>/mockups/` as PlantUML Salt files with `MCK-...` IDs and `@ui-element(UIC-..., role=..., name=..., required=...)` contract comments.

Trace links live in `.mdd/trace.yml`:

```yaml
version: 1
links:
  - from: USE-LOGIN
    to: SEQ-LOGIN
    relation: realizes
generated_tests: []
generated_ui_tests: []
source_links: []
```

Implementation should start from structurally valid models and trace links. Rendered SVGs, `.mdd/approvals.yml`, acceptance tests, and Playwright UI tests are readiness signals; missing or stale readiness data is reported as warnings instead of blocking implementation.

## Rendering

Rendering is handled by the `mdd-render` utility skill, not a public CLI command and not part of the workflow gate. Use a packaged PlantUML jar or `plantuml` on PATH.

Jar rendering uses:

```bash
java -jar path/to/plantuml.jar -tsvg -pipe < .mdd/models/current/use-cases/example.puml > .mdd/rendered/models/current/use-cases/example.svg
```

PlantUML needs Graphviz `dot` for graph-based UML diagrams such as use-case, class, object, component, deployment, state, and legacy activity diagrams. Diagnostic SVG text such as missing Graphviz, `Syntax Error`, or `No diagram found` fails review.

## Workspace

```text
crates/
  mdd-cli      public CLI entrypoint for init, clean, and view
  mdd-core     reusable project state, validation, approvals, trace graph, and mapping internals
  mdd-render   reusable PlantUML rendering adapter
  mdd-agent    legacy/internal agent instruction helper
  mdd-viewer   native egui diagram viewer launched by `mdd view`
```

The non-init workflow is agent-led from generated skills and docs. Removed lifecycle commands such as map, validate, render, approve, test, code, and app are not public CLI commands.
