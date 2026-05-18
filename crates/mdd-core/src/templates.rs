pub struct SkillTemplate {
    pub name: &'static str,
    pub description: &'static str,
    pub body: &'static str,
}

pub const WORKFLOW_SKILLS: &[SkillTemplate] = &[
    SkillTemplate {
        name: "mdd-map",
        description: "Derive the current view of the system from existing code into .mdd/models/current/",
        body: MDD_MAP_SKILL,
    },
    SkillTemplate {
        name: "mdd-generate",
        description: "Derive the objective view of the system from a description into .mdd/models/objective/",
        body: MDD_GENERATE_SKILL,
    },
    SkillTemplate {
        name: "mdd-validate",
        description: "Structural gate over current and objective sides; runs after every map and generate",
        body: MDD_VALIDATE_SKILL,
    },
    SkillTemplate {
        name: "mdd-implement",
        description: "Close the gap between objective and current by writing code; diagrams are refreshed by map afterwards",
        body: MDD_IMPLEMENT_SKILL,
    },
    SkillTemplate {
        name: "mdd-review",
        description: "Two-pass cycle-closure: ID parity + security-marker parity (security-by-default error gate); emits .diff.puml / .security.diff.puml on mismatch",
        body: MDD_REVIEW_SKILL,
    },
    SkillTemplate {
        name: "mdd-render",
        description: "Utility: render PlantUML diagrams to SVG for external visual inspection",
        body: MDD_RENDER_SKILL,
    },
    SkillTemplate {
        name: "mdd-cycle",
        description: "Run a full MDD cycle from one description; owns the cycle boundary, loops to parity, pauses for clarification",
        body: MDD_CYCLE_SKILL,
    },
];

pub fn skill_markdown(skill: &SkillTemplate) -> String {
    format!(
        "---\nname: {}\ndescription: {}\n---\n\n{}",
        skill.name, skill.description, skill.body
    )
}

pub fn mdd_workflow_doc() -> &'static str {
    MDD_WORKFLOW_DOC
}

pub fn uml_and_ocl_guide_doc() -> &'static str {
    UML_AND_OCL_GUIDE_DOC
}

pub fn security_profile_doc() -> &'static str {
    MDD_SECURITY_PROFILE_DOC
}

pub fn claude_entrypoint() -> &'static str {
    CLAUDE_ENTRYPOINT
}

pub fn agents_entrypoint() -> &'static str {
    AGENTS_ENTRYPOINT
}

/// Sentinel that opens the deterministic block `mdd init` injects into
/// `CLAUDE.md` / `AGENTS.md`. `mdd clean` keys off this exact line.
pub const MDD_BLOCK_BEGIN: &str = "<!-- mdd:begin -->";
/// Sentinel that closes the deterministic block.
pub const MDD_BLOCK_END: &str = "<!-- mdd:end -->";
/// Prefix of the JSON metadata comment line that follows the begin sentinel.
pub const MDD_META_PREFIX: &str = "<!-- mdd:meta ";
/// Suffix of the JSON metadata comment line.
pub const MDD_META_SUFFIX: &str = " -->";

const MDD_MAP_SKILL: &str = r#"# MDD Map

You are an MDD, UML, PlantUML, and OCL specialist for the **current** view of the system.

Start by reading `.mdd/docs/mdd-workflow.md` and `.mdd/docs/uml-and-ocl-guide.md`. Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative MDD state.

Use this skill to derive the current view from existing code. Diagrams go into `.mdd/models/current/`.

## Preconditions

- Code must exist for the scope being mapped. If the repository (or the area the user mentioned) is empty, do not run; suggest `/mdd-generate` instead.
- Default scope: produce or refresh the **use-case diagram only**. Extend to other diagram types only when the user asks (e.g. "map domain and components") or when the existing use-case view is already current and they need more detail.

## Workflow

1. Identify the scope: a feature, a module, or the whole repository. Confirm the scope contains code; otherwise stop and report.
2. Inspect the code with fast source search: read build manifests, entrypoints, public APIs, domain modules, tests, routes, commands, and persistence boundaries. Preserve uncertainty with notes in the diagrams rather than inventing behavior.
3. Produce or refine the baseline PlantUML files under `.mdd/models/current/`. The use-case diagram is the foundation; other diagrams reference it via `@ref(...)`:
   - `.mdd/models/current/use-cases/<name>.puml`
   - `.mdd/models/current/sequences/<name>.puml` (only if requested or warranted)
   - `.mdd/models/current/domain/<name>.puml` (only if requested or warranted)
   - `.mdd/models/current/components/<name>.puml` (only if requested or warranted)
   Use one cohesive `<name>` per scope; do not overwrite an unrelated existing baseline.
4. After producing what the user requested, **suggest** any additional diagram types you believe are warranted based on the code structure (e.g. "this code has multiple long-lived stateful entities — would you like state machines under `current/states/`?"). The suggestion is a prompt for the user, not an automatic write.
5. For each domain class produced in step 3, ask: does this class have non-trivial lifecycle behavior — three or more reachable states, transitions guarded by external events, or staleness or approval semantics? If yes, author a state machine under `.mdd/models/current/states/<class>.puml` with `@id(STM-...)`, exactly one `@ref(DOM-...)`, and transition labels naming the use case or skill that drives each transition. If no, note the decision in your output summary.
6. Add stable `@id(...)` markers using the prefixes from `.mdd/docs/uml-and-ocl-guide.md` (`USE-`, `SEQ-`, `DOM-`, `CMP-`, `STM-`). Use `@ref(...)` only between current-side IDs (per-side resolution rule).
7. **Security stereotypes from code** — for each use case, class, sequence, or component in scope, look at the code paths that implement it for evidence of security enforcement. Annotate the current-side element with the matching `@sec(...)` marker reflecting **what the code actually enforces today**, not what it should. See `.mdd/docs/security-profile.md` for full syntax. Per-stereotype brownfield signals:

   - **`<<ByPassing>>`** (host: actor or use case) — route guards, middleware (`requires_auth`, `requires_role("admin")`, `@PreAuthorize`, FastAPI `Depends(...)`, Express middleware, Axum extractor), in-handler role checks. Record `link=<route>`, `allowed=<Role>` (and `denied=<Role>` for explicit rejects). Unauthenticated endpoints that are intentionally public stay unmarked.
   - **`<<Encrypt>>`** (host: class or sequence participant) — TLS termination config, `https://`, `crypto.encrypt(...)`, KMS calls, columns marked `ENCRYPTED BY` in DDL, `@Encrypted` annotations. Record `algorithm=` (e.g. `AES-256-GCM`, `TLS1.3_AES_128_GCM`), `scope=in_transit|at_rest|both`, and `field=<attribute>` when scoped to one field.
   - **`<<BufferOverflow>>`** (host: class) — explicit length checks (`if len(x) > N`), Pydantic `max_length`, Joi `.max(N)`, DB column `VARCHAR(N)`, `#[serde(max_length=N)]`. Record `field=` and `max_length=` (positive int).
   - **`<<SqlInjection>>`** (host: class) — ORM usage or parameterized queries → record with `sanitizer=parameterized` (or `orm`, `prepared-statement`, `escape`, `stored-procedure`). Record `sink=<repository or table>` and `field=<attribute>`. **String concatenation building SQL → do not emit a marker** so `/mdd-review`'s security parity flags the gap.
   - **`<<Flooding>>`** (host: use case or component) — rate-limit middleware (`express-rate-limit`, `tower-governor`, AWS WAF), worker pool sizes, `Semaphore::new(N)`. Record `max_rate=` or `max_concurrent=` (positive int) plus `window=` and optionally `action=throttle|reject|queue`.
   - **`<<Expiration>>`** (host: class) — JWT `exp` claim, session cookie `maxAge`, Redis `TTL`, DB `expires_at` columns. Record `field=<attribute>` and `ttl=<duration>` (e.g. `15m`, `24h`).

   Routes / handlers / fields the description treats as security-sensitive but the code does not enforce: **leave the marker off entirely** so `/mdd-review` surfaces the gap as a missing-marker mismatch.
8. Update `.mdd/trace.yml` with model-to-model trace links and `source_links` from current-side IDs to concrete files or symbols.
9. Hand off to `/mdd-validate`.
"#;

const MDD_GENERATE_SKILL: &str = r#"# MDD Generate

You are an MDD, UML, PlantUML, and OCL specialist for the **objective** view of the system.

Start by reading `.mdd/docs/mdd-workflow.md` and `.mdd/docs/uml-and-ocl-guide.md`. Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative MDD state.

Use this skill to derive the objective view from a description — a POC concept, a feature spec, a change request, or a bug-fix narrative. Diagrams go into `.mdd/models/objective/`.

## Preconditions

- The user must provide a description. If their message has no description, ask for one; do not run without it.
- Code may or may not exist. Generate works either way; it does not require an existing current-side baseline.

## Workflow

1. Read the description. If `.mdd/models/current/` is non-empty, also read those diagrams so the objective refines what exists rather than starting from scratch.
2. Extract actors, externally visible goals, key flows, domain concepts, component boundaries, and UI requirements from the description. Mark uncertainty with notes in the diagrams rather than inventing behavior.
3. Produce or refine PlantUML files under `.mdd/models/objective/`:
   - `.mdd/models/objective/use-cases/<name>.puml`
   - `.mdd/models/objective/sequences/<name>.puml`
   - `.mdd/models/objective/domain/<name>.puml`
   - `.mdd/models/objective/components/<name>.puml`
   - `.mdd/models/objective/states/<name>.puml` (when lifecycle behavior is non-trivial; same rule as map)
4. If the description involves UI, also write PlantUML Salt mockups under `.mdd/models/objective/mockups/<name>.puml` with `MCK-...` IDs and `@ref(...)` markers back to the use case or sequence supported, plus UI contract comments:
   - `@ui-route(/path)`
   - `@ui-viewport(desktop,1280,720)`
   - `@ui-element(UIC-..., role=button, name="Accessible name", required=true)`
   Generate Playwright spec scaffolds under `.mdd/tests/ui/` with `UIT-...` IDs and `framework: playwright`, and link them in `.mdd/trace.yml` under `generated_ui_tests`.
5. Add OCL constraints under `.mdd/constraints/` for domain invariants implied by the description, with `@id(OCL-...)` and `@ref(...)` to the relevant domain model ID.
6. **Security stereotypes** — when the description mentions any security concern, emit the matching `@sec(...)` marker on the affected objective element plus the inline `<<Stereotype>>` on the diagram. See `.mdd/docs/security-profile.md` for full syntax. The marker must live in the same file as its `host=` ID and uses pipe `|` as the list separator.

   - "authentication / login / sign-in / RBAC / admin-only" → `<<ByPassing>>` on the use case (or actor): `host=USE-<NAME>, link=<route>, allowed=<Role1>|<Role2>, denied=<Role>`. For an actor host, `host=ACTOR-<NAME>, role=<Role>`.
   - "encrypted / TLS / at-rest / in-transit / secrets / PII / GDPR sensitive" → `<<Encrypt>>` on the class or sequence participant: `algorithm=<cipher>, scope=at_rest|in_transit|both, field=<attribute>`.
   - "input length / max characters / size limit / bounded input" → `<<BufferOverflow>>` on the class: `field=<attribute>, max_length=<positive int>`.
   - "search / filter / SQL / query / user-supplied database parameter" → `<<SqlInjection>>` on the class: `field=<attribute>, sink=<repository or table>, sanitizer=parameterized|prepared-statement|orm|escape|stored-procedure`.
   - "rate limit / throttle / DoS / max concurrent / requests per second" → `<<Flooding>>` on the use case or component: `max_rate=<n>` and/or `max_concurrent=<n>`, plus `window=<duration>` and optionally `action=throttle|reject|queue`.
   - "session timeout / token TTL / expires after / one-time code" → `<<Expiration>>` on the class: `field=<attribute>, ttl=<duration>`.

   Add `id=SEC-<NAME>` only when the marker is a trace target or test-scaffold host. When the description is silent on a concern for a feature that is plainly out of scope, do not invent a marker — leave the element unmarked rather than emit fabricated values.
7. Add stable `@id(...)` markers using the prefixes from `.mdd/docs/uml-and-ocl-guide.md`. Use `@ref(...)` only between objective-side IDs.
8. Update `.mdd/trace.yml` with model-to-model trace links. Do not add `source_links` here — `/mdd-map` adds those after implementation.
9. Generate acceptance test scaffolds under `.mdd/tests/acceptance/` for use cases that need executable coverage, and link them in `.mdd/trace.yml`.
10. Hand off to `/mdd-validate`.

Keep the objective reviewable and specific. If a behavior is ambiguous, mark the ambiguity in the model and ask for review before treating it as implementation scope.
"#;

const MDD_VALIDATE_SKILL: &str = r#"# MDD Validate

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
"#;

const MDD_IMPLEMENT_SKILL: &str = r#"# MDD Implement

You are an MDD, UML, PlantUML, and OCL specialist for closing the gap between objective and current.

Start by reading `.mdd/docs/mdd-workflow.md` and `.mdd/docs/uml-and-ocl-guide.md`. Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative MDD state.

Use this skill to write code that brings the current state to the objective.

## Preconditions

- `.mdd/models/objective/` must contain at least one diagram.
- `.mdd/models/current/` must be non-empty whenever code already exists (for greenfield POCs, current may be empty — implement writes the first code).
- `/mdd-validate` must have passed since the most recent model edits.

## Workflow

1. Read the objective diagrams under `.mdd/models/objective/` — these describe the intended state.
2. Read the current diagrams under `.mdd/models/current/` — these describe what the code already does (may be empty for greenfield).
3. Compute the gap: which objective `@id`s are not yet present in current. These are the new behaviors, classes, components, or UI elements the code must add. Current `@id`s not in objective represent code that may need removal or migration; default to leaving them alone unless the user asks for removal.
4. Write code changes that close the gap. Do not touch the diagrams in this step — `/mdd-map` refreshes `.mdd/models/current/` after implementation finishes.
5. Keep changes scoped to modeled behavior. Avoid drive-by refactors and feature creep beyond the objective.
6. After code changes are complete, hand off to `/mdd-map` to refresh current, then `/mdd-validate`, then `/mdd-review`.

Readiness warnings (rendered SVGs, approvals, acceptance tests) do not block this skill — report them and continue unless the user asks to pause.
"#;

const MDD_REVIEW_SKILL: &str = r#"# MDD Review

You are an MDD, UML, PlantUML, and OCL specialist for the cycle-closure check.

Start by reading `.mdd/docs/mdd-workflow.md` and `.mdd/docs/uml-and-ocl-guide.md`. Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative MDD state.

Use this skill after a `/mdd-implement → /mdd-map → /mdd-validate` cycle to check whether the current state has caught up to the objective. Review runs **two passes in sequence**; the cycle closes only when both are satisfied.

## Pass 1 — ID parity

1. Build the model registry for both `.mdd/models/current/` and `.mdd/models/objective/`.
2. Compute the gap:
   - **missing**: every `@id` present in objective but absent from current.
   - **extra**: every `@id` present in current but absent from objective (informational; not a hard fail).
3. If `missing` is non-empty → **ID mismatch**. For each objective file containing a missing ID, an annotated `.diff.puml` is written under `.mdd/rendered/review/`; render it via `/mdd-render`. Report the gap and hand off to `/mdd-implement` to close it.

## Pass 2 — security parity

4. Extract `@sec(...)` markers from every non-constraint file on both sides. Key each marker by `(host, stereotype, sorted params)` **excluding** `id=SEC-...`, so two markers with the same body but different SEC- IDs match.
5. Diff objective vs current:
   - **missing security marker**: a marker the objective requires that the current (code-derived) side does not carry — i.e. the design demands a guard the code does not enforce.
   - **extra**: a current-side marker absent from objective (informational).
6. Read `.mdd/config.yml` → `security.parity_check`:
   - **`error` (default — security-by-default)**: a missing security marker **blocks cycle closure exactly like an ID mismatch**.
   - **`warn`**: missing markers are reported prominently but do not by themselves block closure (opt-down for projects not yet enforcing security parity).
7. On any missing security marker, `.mdd/rendered/review/<diagram>.security.diff.puml` is written (render via `/mdd-render`). Report the gap and hand off to `/mdd-implement`.

## Closure rule

The cycle is **done** only when **ID parity matches AND (security parity matches OR `security.parity_check` is `warn` and the user accepts the listed warnings)**. ID parity is strict structural; the security gate is `error` by default. The user may override a result manually (e.g. "ignore this extra"), but the default decision is automatable from `Project::review()`.
"#;

const MDD_RENDER_SKILL: &str = r#"# MDD Render

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
8. Also synthesize a PlantUML constraints diagram from every `.mdd/constraints/*.ocl` and rasterize it to `.mdd/rendered/constraints/<name>.svg` (via `mdd_render::render_ocl_diagrams`) so the viewer's OCL Diagram sub-mode can paint it.
9. Report the list of rendered files and any diagnostic failures. The user reviews them externally.

Rendering is not a gate. Validation, implementation, and review do not depend on a render pass.
"#;

const MDD_CYCLE_SKILL: &str = r#"# MDD Cycle

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
"#;

const MDD_WORKFLOW_DOC: &str = r#"# MDD Workflow

`mdd init` bootstraps an agent-first model-driven development workspace. The public CLI intentionally stops there. Claude Code and Codex project skills drive the entire model lifecycle: mapping code, generating objectives, validating, implementing, and reviewing the loop closure.

Authoritative files:

- `.mdd/models/current/` and `.mdd/models/objective/` contain UML models written as PlantUML.
- `.mdd/constraints/` contains OCL constraints (shared across both sides; OCL references the domain ID on either side).
- `.mdd/trace.yml` contains trace links between models, tests, and source.
- `.mdd/approvals.yml` records optional review approvals for current model and constraint file hashes.

## Two-sided model layout

The workspace tracks two parallel views of the system:

- **Current** (`.mdd/models/current/`): what the codebase already does. Produced by `/mdd-map` from existing code.
- **Objective** (`.mdd/models/objective/`): what the codebase should do. Produced by `/mdd-generate` from a description.

Each side has the same six diagram-type subdirectories:

- `<side>/use-cases/` — use-case diagrams (actors, user goals, externally visible behavior).
- `<side>/sequences/` — sequence diagrams (runtime interactions, command/query/integration flows).
- `<side>/domain/` — class or object diagrams (entities, value objects, relationships, invariants referenced by OCL).
- `<side>/components/` — component diagrams (packages, services, deployable units, adapters, ownership boundaries).
- `<side>/mockups/` — PlantUML Salt mockup diagrams with UI contracts (authored by `/mdd-generate` when the description involves UI).
- `<side>/states/` — state-machine diagrams for domain classes with non-trivial lifecycle.

An `@id(USE-X)` may appear in both `current/use-cases/...` and `objective/use-cases/...` — it represents the same logical use case in two states. Validation enforces uniqueness within each side.

## Workflow cycle

Two entry points and five productive skills, plus one render utility:

```
ENTRY-A (existing code):   /mdd-map      ->  /mdd-validate  ->  /mdd-generate  -> ...
ENTRY-B (description):     /mdd-generate ->  /mdd-validate  -> ...

After /mdd-map or /mdd-generate, /mdd-validate gates progression.
When both sides are non-empty and validate is clean, /mdd-implement may run.

Implement cycle:
  /mdd-implement (writes code)
      -> /mdd-map      (refreshes current from new code)
        -> /mdd-validate
          -> /mdd-review
              | match    -> DONE  (start the next cycle)
              | mismatch -> /mdd-implement (loop)
```

`/mdd-render` is a utility: it renders any `.puml` under `.mdd/models/` to SVG on demand for external inspection. It is **not** part of the gate and does not block any other skill.

## Orchestrated entry point

`/mdd-cycle` runs the whole loop from a single description. It selects the entry point (`/mdd-generate` when a description is given, otherwise it behaves as `/mdd-map` with no comments and stops), and it **owns the cycle boundary**: it opens a numbered cycle under `.mdd/cycles/<N>/`, snapshots `.mdd/models/current/` to `before/`, loops to parity, then on review match snapshots `after/`, writes annotated `<diagram>.diff.puml` files, and closes the manifest. Standalone `/mdd-map` and `/mdd-generate` never open or close a cycle. Whenever a decision is genuinely ambiguous, `/mdd-cycle` pauses and asks the user — it never guesses. The viewer reads `.mdd/cycles/` to group diagrams by cycle and render the superposed before/after diff.

## Whole-map baseline

`/mdd-cycle` keeps a project-wide **whole-map** under `.mdd/map/` — a per-concept picture of the whole system that grows cycle by cycle. It is maintained only by the cycle **Close** step: after the cycle's `<diagram>.diff.puml` files are written, that cycle's `CycleDiff` is folded into `.mdd/map/<kind>/<name>.puml` (added `@id`s copied in from `after/` and tagged with a `' @cycle(<ID>, <N>)` provenance line, removed `@id`s deleted, unchanged ones keeping their earlier provenance). It is never re-derived from code and there is no `/mdd-map` "whole" mode; accumulation is one cheap diff application per cycle, so an element added by one cycle and removed by a later one nets to **neither** (no `<<removed>>` ghost, unlike a single cycle's `.diff.puml`). `.mdd/map/manifest.yml` records `version`, `last_cycle`, `generated_at`, and `files`. The whole `.mdd/map/` tree is snapshotted into `.mdd/cycles/<N>/whole/` at close so the picture *as of cycle N* is recoverable, and `/mdd-render` rasterizes `.mdd/map/**.puml` to `.mdd/rendered/map/**.svg`.

The whole-map is an inspection artifact **outside the parity gate**: `/mdd-validate`, `/mdd-review`, and the `/mdd-cycle` parity loop never read or gate on `.mdd/map/`. The `OCL-MAP-*` constraints in `.mdd/constraints/whole-map.ocl` describe the artifact's invariants but are not parity checks. Greenfield (no closed cycle) means no `.mdd/map/` tree at all.

## ID And Ref Conventions

Every PlantUML model file must contain at least one stable`@id(...)` marker. Significant model elements should also have IDs when they need traceability, review, testing, or implementation links.

Use readable prefixes:

- `USE-...` for use cases.
- `SEQ-...` for sequences.
- `DOM-...` for domain concepts.
- `CMP-...` for components.
- `STM-...` for state machines.
- `MCK-...` for mockups.
- `UIC-...` for UI contract elements.
- `UIT-...` for generated UI tests.
- `OCL-...` for constraints.
- `AT-...` for acceptance tests.

Use `@ref(...)` when a diagram or OCL constraint depends on another model ID. **Refs resolve within the same side**: a current-side `@ref(USE-X)` must point to a current-side `USE-X`; an objective-side `@ref(USE-X)` must point to an objective-side `USE-X`. OCL constraints may reference domain IDs on either side.

Examples:

```plantuml
@startuml
' @id(USE-LOGIN)
actor User
usecase "Log in" as Login
@enduml
```

```plantuml
@startuml
' @id(SEQ-LOGIN)
' @ref(USE-LOGIN)
actor User
participant App
User -> App: submit credentials
@enduml
```

```ocl
-- @id(OCL-USER-EMAIL-REQUIRED)
-- @ref(DOM-USER)
context User
inv EmailRequired: self.email <> ''
```

PlantUML Salt mockups use `@startsalt` and structured UI contract comments:

```plantuml
@startsalt
' @id(MCK-CHECKOUT-FORM)
' @ref(USE-CHECKOUT)
' @ui-route(/checkout)
' @ui-viewport(desktop,1280,720)
' @ui-element(UIC-CHECKOUT-SUBMIT, role=button, name="Submit order", required=true)
{
  Email | "user@example.com"
  [Submit order]
}
@endsalt
```

## Trace Rules

`.mdd/trace.yml` uses durable trace links:

```yaml
version: 1
links:
  - from: USE-LOGIN
    to: SEQ-LOGIN
    relation: realizes
generated_tests:
  - id: AT-USE-LOGIN
    path: .mdd/tests/acceptance/use-login.feature
    model_id: USE-LOGIN
generated_ui_tests:
  - id: UIT-MCK-LOGIN-FORM
    path: .mdd/tests/ui/mck-login-form.spec.ts
    model_id: MCK-LOGIN-FORM
    framework: playwright
source_links:
  - model_id: DOM-USER
    path: src/domain/user.rs
    symbol: User
```

Additional relations used by state-machine diagrams:

```yaml
links:
  - from: STM-USER-LIFECYCLE
    to: DOM-USER
    relation: models_lifecycle_of
  - from: USE-LOGIN
    to: STM-USER-LIFECYCLE
    relation: triggers_transition
```

Rules:

- Every link must reference existing `@id(...)` values. A link may cross sides (e.g. an objective USE that ties to a current SEQ) but typically both endpoints live in the same side.
- Every use case intended for implementation should trace to at least one sequence diagram.
- Every state-machine diagram is linked to the domain class whose lifecycle it describes with `relation: models_lifecycle_of`. Each use case or skill that drives a transition in the state machine is linked with `relation: triggers_transition`.
- `generated_tests` must point to existing acceptance test files.
- `generated_ui_tests` must point to existing Playwright spec files and reference mockup IDs.
- `source_links` must point to existing source files and should include a symbol when practical.
- When code moves, update source links in the same change.

## Validation Checklist

`/mdd-validate` walks both `.mdd/models/current/` and `.mdd/models/objective/` and checks:

- UML and PlantUML files are in `.mdd/models/current/<kind>/` or `.mdd/models/objective/<kind>/`.
- Every PlantUML model has at least one `@id(...)`.
- IDs are unique **within the same side**. The same ID may appear in current and objective (it represents the same logical model in different states).
- Every `@ref(...)` resolves: current-side refs to current-side IDs; objective-side refs to objective-side IDs. OCL files may reference domain model IDs in either side.
- OCL constraints live under `.mdd/constraints/` and reference domain model IDs.
- Mockup files under `<side>/mockups/` include at least one `MCK-...` ID.
- UI contract element IDs from `@ui-element(...)` are unique `UIC-...` IDs across the workspace.
- Implementation-ready mockups (those with `@ui-route(...)` and at least one `@ui-element(...)`) have generated Playwright UI tests linked in `generated_ui_tests`.
- State-machine files under `<side>/states/` include at least one `STM-...` ID and exactly one `@ref(DOM-...)`.
- Every `@sec(...)` marker parses, declares a stereotype in the active catalog (currently `ByPassing`, `Encrypt`, `BufferOverflow`, `SqlInjection`, `Flooding`, `Expiration`), has a `host=` that resolves to a same-side `@id(...)` in the same file on a host kind the stereotype accepts, and supplies the tagged values its stereotype requires (see `.mdd/docs/security-profile.md`). Unknown stereotypes fail validation.
- Trace links in `.mdd/trace.yml` reference existing IDs and every use-case ID traces to at least one sequence ID.
- Acceptance tests that exist are linked in trace data.
- Approval hashes in `.mdd/approvals.yml` match the current model and constraint files when review metadata is present.

Validation errors block the next skill until fixed. Missing or stale approvals, rendered SVGs, and acceptance-test coverage are readiness warnings; report them and continue unless the user asks to pause.

## Review Closure

`/mdd-review` runs after `/mdd-implement -> /mdd-map -> /mdd-validate` and performs **two passes**; the cycle closes only when both are satisfied (`Project::review()` computes the combined gate).

**Pass 1 — ID parity.** Compare the current and objective `@id` sets:

- **Match** (no missing IDs): ID parity is satisfied.
- **Mismatch** (one or more objective IDs absent from current): annotated `.diff.puml` files are written under `.mdd/rendered/review/<diagram>.diff.puml` (missing in green `<<missing>>`, extras in red `<<extra>>`). Hand off back to `/mdd-implement`.

**Pass 2 — security parity.** Diff `@sec(...)` markers (keyed by host + stereotype + sorted params, excluding `id=SEC-...`) between objective and current. A **missing security marker** means the objective requires a guard the current (code-derived) side does not enforce. Behavior depends on `.mdd/config.yml` `security.parity_check`:

- **`error` (default — security-by-default)**: a missing security marker blocks cycle closure exactly like an ID mismatch.
- **`warn`**: missing markers are reported and `.mdd/rendered/review/<diagram>.security.diff.puml` is written, but they do not by themselves block closure (opt-down).

**Closure rule**: the cycle is complete only when ID parity matches **and** (security parity matches **or** `security.parity_check` is `warn` and the user accepts the warnings). The user may override a mismatch manually if a particular extra or missing element is intentional.

## Rendering On Demand

`/mdd-render` produces SVGs under `.mdd/rendered/<source-path>.svg` for any PlantUML file. Use the packaged PlantUML jar where available:

```bash
java -jar path/to/plantuml.jar -tsvg -pipe < .mdd/models/current/use-cases/example.puml > .mdd/rendered/models/current/use-cases/example.svg
```

PlantUML needs Graphviz `dot` for graph-based UML diagrams (use-case, class, object, component, deployment, state, and legacy activity diagrams). If `dot` is installed outside PATH, set `GRAPHVIZ_DOT=/path/to/dot`.

Rendering reports any PlantUML diagnostic text (`Dot executable does not exist`, `Cannot find Graphviz`, `Syntax Error`, `Error`, `No diagram found`) but does not block any workflow step. The user reviews SVGs in an external editor.

## Review Readiness (optional approval)

Approval is explicit user confirmation of the **current** models and constraints. After the user approves, update `.mdd/approvals.yml` with `approved: true`, an `approved_at` timestamp, and the SHA-256 hash of every `.puml`, `.plantuml`, `.uml`, and `.ocl` file under `.mdd/models/current/` and `.mdd/constraints/`.

Implementation readiness is reported from:

- structural validation errors;
- `.mdd/approvals.yml` freshness;
- affected use-case acceptance tests under `.mdd/tests/acceptance/`;
- affected UI mockup Playwright tests under `.mdd/tests/ui/`;
- `.mdd/trace.yml` links between models, tests, and source for the requested change.

Structural validation errors should be fixed before implementation. Missing or stale approvals and acceptance tests are readiness warnings; report them and continue unless the user asks to pause.
"#;

const UML_AND_OCL_GUIDE_DOC: &str = r#"# UML And OCL Guide

Use PlantUML for UML diagrams and OCL for constraints. The goal is not exhaustive modeling; it is durable, reviewable intent that agents can validate before coding.

The workspace has two sides — `current/` (what the code does) and `objective/` (what the code should do) — and every diagram type below may live in **either side** under `.mdd/models/<side>/<kind>/`. The path examples in each section use `<side>`; substitute `current` or `objective` based on the skill writing the file (`/mdd-map` writes to `current/`, `/mdd-generate` writes to `objective/`).

## Use-Case Diagrams

Place use-case diagrams in `.mdd/models/<side>/use-cases/`. Each file should include actors, externally visible goals, and one or more stable `@id(...)` markers. Use `USE-...` IDs.

Keep use cases user-centered. Avoid encoding implementation steps that belong in sequence diagrams.

## Sequence Diagrams

Place sequence diagrams in `.mdd/models/<side>/sequences/`. Each use case intended for implementation should have at least one sequence diagram with an `@id(...)` and an `@ref(...)` to the use-case ID it realizes (same side).

Show important participants, calls, events, alternate flows, and failure paths. Avoid drawing every private helper call.

## Domain Diagrams

Place domain diagrams in `.mdd/models/<side>/domain/`. Use class or object diagrams for entities, value objects, services with domain meaning, relationships, multiplicities, and important attributes. Domain IDs use `DOM-...`.

OCL constraints should reference domain IDs, so keep domain model names stable and aligned with the ubiquitous language of the repository.

## Component Diagrams

Place component diagrams in `.mdd/models/<side>/components/`. Use these for packages, services, adapters, UI shells, persistence layers, external systems, and deployable units. Component IDs use `CMP-...`.

Component diagrams should make ownership and dependencies clear enough for implementation planning.

## State Machine Diagrams

Place state-machine diagrams in `.mdd/models/<side>/states/`. Use them when a domain class has non-trivial lifecycle behavior — three or more reachable states, transitions guarded by external events, or staleness and approval semantics that use-case `<<gates>>` arrows cannot express crisply. State-machine IDs use `STM-...` and must reference exactly one domain class via `@ref(DOM-...)`.

PlantUML syntax:

```plantuml
@startuml
' @id(STM-USER-LIFECYCLE)
' @ref(DOM-USER)
[*] --> Pending
Pending --> Active : verify-email
Active --> Suspended : flag-abuse
Suspended --> Active : reinstate
Active --> [*] : delete
@enduml
```

Transition labels should name the trigger that causes the transition — a use-case ID, a skill name, or a domain event — so the diagram is the canonical record of what drives each state change. Keep state names user-meaningful (`Approved`, `StaleAfterEdit`) rather than implementation-internal.

Link the state machine in `.mdd/trace.yml` with `relation: models_lifecycle_of` from `STM-...` to its `DOM-...`, and with `relation: triggers_transition` from each named use case or skill to the `STM-...`.

## Mockup Diagrams

Place PlantUML Salt mockups in `.mdd/models/<side>/mockups/` (typically `objective/mockups/`, since UI mockups are authored by `/mdd-generate` from a description). Mockup IDs use `MCK-...`, UI contract element IDs use `UIC-...`, and generated UI tests use `UIT-...`.

Mockups should reference the use case or sequence they support with `@ref(...)` (same side). Use structured comments to define route, viewport, roles, accessible names, required states, and primary actions:

```plantuml
@startsalt
' @id(MCK-CHECKOUT-FORM)
' @ref(USE-CHECKOUT)
' @ui-route(/checkout)
' @ui-viewport(desktop,1280,720)
' @ui-element(UIC-CHECKOUT-SUBMIT, role=button, name="Submit order", required=true)
{
  [Submit order]
}
@endsalt
```

Mockups with both `@ui-route(...)` and at least one `@ui-element(...)` are implementation-ready and must have a generated Playwright spec linked from `.mdd/trace.yml` under `generated_ui_tests`.

## OCL Constraints

Place OCL files in `.mdd/constraints/` (shared across sides). Each constraint file should include `@id(...)` for important constraints and `@ref(...)` to the domain model ID it constrains. The `@ref(DOM-...)` may resolve in either side; typically OCL constrains the current-side domain model since OCL describes runtime invariants.

Example:

```ocl
-- @id(OCL-ORDER-TOTAL-NONNEGATIVE)
-- @ref(DOM-ORDER)
context Order
inv TotalNonNegative: self.total >= 0
```

Reference rules:

- OCL `context` names must match domain model concepts.
- OCL files should not reference use-case, sequence, or component IDs as their primary constrained element.
- If a constraint supports a use case, express that through `.mdd/trace.yml` trace links instead of replacing the domain `@ref(...)`.

## PlantUML Notes

Use comments for IDs and refs so diagrams remain valid PlantUML:

```plantuml
' @id(DOM-USER)
' @ref(USE-LOGIN)
```

Keep aliases stable when other diagrams or notes refer to them. Prefer readable labels and explicit relationships over hidden inference.

## Security Stereotypes

Security-sensitive use cases, sequences, classes, and components carry inline UML stereotypes plus `@sec(...)` comment markers that record tagged values. The marker must live in the same file as the `@id(...)` it references via `host=`. The full profile, per-stereotype tagged-value contracts, and accepted host kinds live in `.mdd/docs/security-profile.md`. `SEC-...` (security requirement / annotation host) and `THR-...` (misuse case / threat) are reserved ID prefixes; SEC- IDs follow the same per-side uniqueness rule as other `@id(...)` values.

The active stereotypes (Peralta OWASP-derived catalog):

- `<<ByPassing>>` — access-control bypass (host: actor or use case).
- `<<Encrypt>>` — field or channel encryption (host: class or sequence participant).
- `<<BufferOverflow>>` — bounded-input length guard (host: class).
- `<<SqlInjection>>` — bound SQL parameter with sanitizer (host: class).
- `<<Flooding>>` — rate or concurrency limit (host: use case or component).
- `<<Expiration>>` — session/token TTL (host: class).

Example on a use case:

```plantuml
@startuml
' @id(USE-CHANGE-BOOK-PRICE)
' @sec(stereotype=ByPassing, host=USE-CHANGE-BOOK-PRICE, link=/admin/books, allowed=Admin, denied=Anonymous|Customer, id=SEC-ADMIN-PRICE-GUARD)
usecase "Change book price" as ChangePrice <<ByPassing>>
@enduml
```

Example on a domain class with a bounded-input field:

```plantuml
@startuml
' @id(DOM-USER-INPUT)
' @sec(stereotype=BufferOverflow, host=DOM-USER-INPUT, field=email, max_length=254, id=SEC-EMAIL-LEN)
class UserInput <<BufferOverflow>> {
  + email : String
}
@enduml
```

## Consistency Rules

- Every significant behavior starts in a use case.
- Every use case intended for implementation traces to a sequence.
- UI-facing behavior should have a Salt mockup when route, layout, or accessible interaction contracts matter — authored by `/mdd-generate` into `.mdd/models/objective/mockups/`.
- Domain behavior with invariants has OCL constraints.
- Components that own behavior are linked through trace data.
- Security-sensitive behavior carries `@sec(...)` markers per the security profile.
- Validation errors should be fixed before implementation; readiness warnings (rendering, approvals, acceptance-test gaps) do not block.
- Refs resolve **within the same side**: never write a `@ref(USE-X)` inside `current/` and expect it to resolve to an objective `USE-X`.
"#;

const CLAUDE_ENTRYPOINT: &str = r#"# Claude Code MDD Entry Point

This repository uses agent-first MDD. Start by reading `.mdd/docs/mdd-workflow.md` and `.mdd/docs/uml-and-ocl-guide.md`.

Workflow skills in `.claude/skills/`:

- `/mdd-map` — derive current view from existing code into `.mdd/models/current/`.
- `/mdd-generate` — derive objective view from a description into `.mdd/models/objective/`.
- `/mdd-validate` — structural gate over current and objective sides.
- `/mdd-implement` — close the gap between objective and current by writing code.
- `/mdd-review` — strict structural match between current and objective; emits annotated diff PUMLs on mismatch.

Orchestration skill (runs the whole loop from one description):

- `/mdd-cycle` — selects the entry point, owns the cycle boundary under `.mdd/cycles/`, loops to parity, and pauses for clarification.

Utility skill (on demand, not a workflow gate):

- `/mdd-render` — render PlantUML diagrams to SVG for external visual inspection.

Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative planning context. Validate IDs, refs, and trace links before implementation; report missing rendering, approval, or acceptance-test readiness as warnings instead of blocking implementation.
"#;

const AGENTS_ENTRYPOINT: &str = r#"# Agent MDD Entry Point

This repository uses agent-first MDD. Start by reading `.mdd/docs/mdd-workflow.md` and `.mdd/docs/uml-and-ocl-guide.md`.

Codex and other agents should use the project skills in `.codex/skills/`:

- `/mdd-map` — derive current view from existing code into `.mdd/models/current/`.
- `/mdd-generate` — derive objective view from a description into `.mdd/models/objective/`.
- `/mdd-validate` — structural gate over current and objective sides.
- `/mdd-implement` — close the gap between objective and current by writing code.
- `/mdd-review` — strict structural match between current and objective; emits annotated diff PUMLs on mismatch.
- `/mdd-cycle` — orchestration: run the whole loop from one description; owns the cycle boundary, loops to parity, pauses for clarification.
- `/mdd-render` — utility: render PlantUML diagrams to SVG for external visual inspection.

Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative planning context. Validate IDs, refs, and trace links before implementation; report missing rendering, approval, or acceptance-test readiness as warnings instead of blocking implementation.
"#;

const MDD_SECURITY_PROFILE_DOC: &str = include_str!("../../../.mdd/docs/security-profile.md");
