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
    SkillTemplate {
        name: "mdd-deploy",
        description: "Utility: PLAN then EXECUTE an Azure Container Apps deployment — generates a UML deployment diagram, runbook, and Bicep or Terraform IaC (operator-confirmed dialect + purpose), then runs the runbook to live traffic: manages auth, dry-runs, applies, provisions, migrates, and routes traffic, pausing only at irreversible steps and a never-auto-confirmed go-live gate; halts on first error. Surfaces ungrounded ambiguity instead of guessing; outside the parity gate",
        body: MDD_DEPLOY_SKILL,
    },
    SkillTemplate {
        name: "mdd-ralph",
        description: "Utility: run the Ralph loop — an unattended self-paced loop (driven by /loop) that picks one highest-priority item from a plan pointer (.mdd/ralph/PLAN.md by default; any source may write it) each iteration, routes it to the right MDD skill or general tools, and must pass the parity gate before committing; runs on a feature branch, exits on RALPH-DONE; outside the parity gate, on-demand",
        body: MDD_RALPH_SKILL,
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

pub fn deploy_profile_doc() -> &'static str {
    MDD_DEPLOY_PROFILE_DOC
}

pub fn ralph_prompt() -> &'static str {
    RALPH_PROMPT
}

pub fn ralph_plan() -> &'static str {
    RALPH_PLAN
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
/// The command the mdd-managed Claude Code SessionStart hook runs
/// (DOM-SESSION-HOOK / CMP-INIT-HOOK). `mdd init` merges a hook running this
/// into `.claude/settings.json`; `mdd clean` removes exactly it.
pub const SESSION_HOOK_COMMAND: &str = "mdd context";

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

## Run the deterministic gate

The structural checks are implemented and tested in `Project::validate()` and exposed as a CLI command — **run it, do not re-derive the checklist by hand**:

```bash
mdd validate --json
```

It prints a slim `{ "ok": bool, "errors": [...], "warnings": [...] }` object. Interpret it:

- **`ok: true`** → the structural gate passes. Surface `warnings` as readiness notes (approvals, acceptance tests, rendered SVGs) and unlock the next step.
- **`ok: false`** → the gate fails. Relay each `errors` entry, stop, and fix the offending diagram or trace data before running any other skill.

`mdd validate` (text mode) is the human-readable equivalent and exits non-zero on a blocking error. It is independent of `mdd review` — it never runs the parity gate. Use the checklist below to understand and explain what the engine enforces; the command is the source of truth.

Validation checklist (what `mdd validate` enforces):

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

If `mdd validate` reports `ok: true` (no errors), unlock the next step in the workflow:
- After `/mdd-map` or `/mdd-generate`, the user may run either skill again or `/mdd-implement` if both sides have content.
- After the post-implement `/mdd-map`, hand off to `/mdd-review`.

If it reports `ok: false`, stop and fix the structural errors in the affected diagrams or trace data before running any other skill.

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

const MDD_DEPLOY_SKILL: &str = r#"# MDD Deploy

You are an MDD, UML, PlantUML, and OCL specialist for deployment planning and execution.

This is a **utility skill, NOT a workflow gate** — a sibling of `/mdd-render`. It PLANS a deployment (a UML deployment diagram, generated Infrastructure-as-Code, and an explicit runbook) **and then EXECUTES that runbook all the way to live traffic** — managing cloud auth, dry-running, applying, provisioning secrets, running the migration, and routing traffic, via `az` / `terraform` / `docker`. `/mdd-validate`, `/mdd-implement`, `/mdd-review`, and the `/mdd-cycle` parity loop do not depend on it and never read `.mdd/deploy/`. A deployment has no code-derived counterpart, so executing it (not merely guiding it) does not pull it into the parity gate.

Read `.mdd/docs/deploy-profile.md` first — it defines the deployment-diagram conventions, the `azure-container-apps` node vocabulary, the invariant checklist, the deployment-purpose and access-completeness gates, the generalized landmine rule, and the **execution model** (auth, dry-run, the autonomous-vs-irreversible step classification, halt-on-error, and the go-live gate) this skill must preserve.

## The adaptive habit (read before the steps)

Most of this skill's failures share one shape: it confidently generates something whose correctness depends on a fact it never grounded. Two guards run against that, and they **generalize** — do not treat them as a fixed list of known traps:

- **Ground before you generate.** Every choice whose correctness depends on a fact must trace to an input you actually read (`.mdd/models`, the target repo, the operator's answers). If it is a default you filled in, it is ungrounded — **surface it, do not bake it in.** This is the generalized landmine rule (step 3); it covers the app-config axis (a default vs. shipped code), the **purpose axis** (dev vs. prod — step 5), the **access-path axis** (who reaches a secured store — step 6), and any other axis with the same shape.
- **Every secured store has two ends, and a locked door needs a key-holder who can reach it.** For each secured resource you draw, ask: who **reads** it, who **writes / provisions** it, and — after any hardening you apply — can each still reach it? A store read but never written, or a writer with no admitted path, is an incomplete diagram, not a finished one (step 6).

Where any of this is undetermined, **pause and surface the choice** for the operator — never jump to a solution. A paused, correct deployment beats a fast, broken one. **This discipline extends into execution: pause at every irreversible step and halt on the first failure rather than barrel ahead.**

## Workflow — Phase A: PLAN (steps 1-10)

The plan phase produces the artifacts (diagram + IaC + runbook) under the gates below; it is unchanged from the guidance-only skill. Phase B then executes the runbook.

1. Read the deployment description. v1 supports exactly one target: `azure-container-apps` (the sibling repo `../atlas-ate-server`). If any other target is requested, say it is not yet supported and stop. Also read which **IaC dialect** is requested — exactly one of `bicep` or `terraform` per run (these are the only supported dialects).
2. Read context, **read-only**: `.mdd/models/**/{components,use-cases}/*.puml` (the *what* of the system) and the target repo `../atlas-ate-server` (`README.md`, `Dockerfile`, `docker-compose.yml`, `.env.example`, `src/`) to ground the topology and security invariants in reality.
3. **Generalized landmine pause — mandatory.** Whenever a topology, sizing, security, or access choice is genuinely ambiguous, **STOP and ask the user** before proceeding — the same discipline as `/mdd-cycle`. Never guess. This pause is mandatory for any **go-live landmine**: any choice whose correctness depends on a fact NOT grounded in the inputs you read (`.mdd/models`, the target repo `src/`, `Dockerfile`, `.env.example`, the operator's answers). It is **not a fixed list of traps — recognize the shape.** Three axes recur: (a) **app-config** — a generated config default contradicting what the shipped code supports (worked example: defaulting Azure OpenAI to managed identity while the server authenticates only with an API key — no `@azure/identity` code path — would ship an app that cannot reach Azure OpenAI at go-live); (b) **purpose** — sizing or posture chosen without knowing dev vs. prod (step 5); (c) **access-path** — a secured store hardened without a way in for whoever must reach it (step 6). Surface it as a blocking question — **never bury it as a runbook STOP note.** See `.mdd/docs/deploy-profile.md` ("Landmine detection — mandatory pause").
4. **Resolve and CONFIRM the IaC dialect — before any artifact.** The dialect is exactly one of `bicep` or `terraform`, one per run. There is **no silent default**: if the description does not unambiguously name the dialect — and, either way, to confirm the resolved choice — **STOP and ask the user for an explicit confirmation** before writing ANY deploy artifact (the deployment diagram, the generated IaC, or the runbook). This blocking confirmation is mandatory on every run, uses the same discipline as the landmine pause, and is never demoted to a runbook STOP note. Do not create `.mdd/deploy/` or any `infra/` file until the operator has confirmed the dialect.
5. **Resolve and CONFIRM the deployment purpose — before any purpose-driven default.** Establish whether this deployment is **dev, staging, or prod** and **STOP and ask the user to confirm** — the same blocking discipline as the dialect. **No silent default — never assume prod-grade or dev-grade.** The confirmed purpose drives the defaults that hang on it: Azure Database for PostgreSQL **tier / redundancy** (e.g. a burstable tier for dev vs. general-purpose / HA for prod), backup retention, and which network / doorway posture is **recommended**. **Surface** the purpose-appropriate options for the operator to confirm; do not bake one in. Secure-by-default is preserved — the vault stays most-restrictive and any relaxation is the explicit, surfaced decision of step 6.
6. **Access-completeness pass — before any artifact.** For every secured store in the topology (Azure Key Vault, Azure Database for PostgreSQL, the container registry, the remote state backend), name **who READS it** and **who WRITES / provisions it**, and give each an **identity** and a **network path the chosen posture actually admits**. Typically the running app is the reader and the **deployer** (the operator or CI that runs `apply`) is the writer. A store that is **read but never written** (secrets nobody is shown provisioning), or a **writer with no admitted path** through a door you just hardened, is an incomplete diagram — **STOP and surface it** with the connectivity options (allowlist the deployer's IP, provision from inside the VNet, or a tunnel), framed by the deployment purpose. **Do not pick the option for the operator.** The diagram is not "done" until every secured store has both ends with an admitted path.
7. Write `.mdd/deploy/azure-container-apps/diagram.puml`: a true UML **deployment** diagram — nodes (Azure Container Registry, Container Apps environment + the app revision, Azure Database for PostgreSQL, Azure Key Vault, Azure OpenAI), the deployed artifact (the server container image), and annotated communication paths/protocols. Draw **both** the runtime read paths AND the provisioning write paths surfaced in step 6 — e.g. the deployer writing secrets into Key Vault, annotated with its write role, alongside the app's read role. Reuse the stereotype vocabulary (`<<Encrypt>>`, `<<ByPassing>>`, `<<Flooding>>`, `<<Expiration>>`) as **documentation-only** PlantUML stereotypes/notes. These are NOT the gated security-marker mechanism: do not write `@sec` markers here.
8. Generate the IaC in the **confirmed dialect only** (one per run), into the target repo `../atlas-ate-server/infra/`:
   - **Bicep** → `infra/main.bicep` (+ Bicep modules).
   - **Terraform** → `infra/main.tf` (+ Terraform modules), the `azurerm` provider, and a **remote state backend** (`backend "azurerm"`).
   Either dialect declares the identical resource set: Container Apps, ACR, Azure Database for PostgreSQL (TLS required; **tier sized to the confirmed purpose** from step 5), Key Vault + secret references, external ingress on `8080`, and the database migration as a separate Container Apps job run before traffic routing. Emit **every access grant the step-6 pass surfaced** — the reader's read-only role AND the writer / deployer's write role — plus whatever connectivity the operator chose. Cross-repo output is fine — the skill is non-gated. **Secure-by-default network posture — full parity, identical for both dialects**: automatable hardening is not a deferred human decision. Never emit a secret or data store more openly network-reachable than its peers; choose the most restrictive network posture consistent with the connectivity the runbook actually requires — **runtime AND provisioning** — and relax it only via the explicit, surfaced decision from step 6. Concretely for the v1 target, when Postgres and Azure OpenAI are private, Key Vault must default to the most restrictive posture — Bicep `networkAcls.defaultAction: 'Deny'` + `bypass: 'AzureServices'`, the Terraform equivalent `network_acls { default_action = Deny, bypass = AzureServices }` (or a private endpoint) — never public network access with an `Allow` default. See `.mdd/docs/deploy-profile.md` ("Secure-by-default").
9. Write `.mdd/deploy/azure-container-apps/runbook.md`: numbered steps for the confirmed dialect, each with the exact command, the directory / Azure context to run it in, the required env/secret values, and an explicit **STOP / confirm** marker before every irreversible or go-live step. The secret-provisioning step must state the two real preconditions surfaced in step 6 — the deployer holds the data-plane write role AND has a network path the posture admits — and note the role-assignment propagation delay. These STOP markers are exactly the points where Phase B pauses for confirmation; the idempotent steps between them the executor runs autonomously.
10. Enforce, in the diagram and the runbook, **identically regardless of the confirmed dialect**, every `../atlas-ate-server` invariant from `.mdd/docs/deploy-profile.md`: Key-Vault-only secrets, App Attest required, the billing production multi-factor gate, BYOK never touching the server, DB TLS + at-rest encryption, the pre-traffic migration job, non-root container, port `8080` ingress.
11. **Plan complete — proceed to execution.** The runbook you just wrote is now the **execution plan you walk yourself** (Phase B). Do not stop here. (`/mdd-render` rasterizes `.mdd/deploy/**/*.puml` to `.mdd/rendered/deploy/**/*.svg` for visual inspection at any time.)

## Workflow — Phase B: EXECUTE the runbook

Walk the runbook top to bottom via Bash (`az`, `terraform`, `docker`). Two rules govern the whole phase:

- **Halt on the first failed step.** If any command fails, STOP, report the failing step and its output, and run no further step. Never barrel ahead on a broken deploy. (OCL-DEPLOY-EXEC-HALT-ON-ERROR)
- **Pause only at irreversible steps.** Idempotent / reversible steps (resource-group + ACR create, image build & push, `plan` / `what-if`, validation, health checks) run **autonomously**; the operator confirms only at irreversible steps. (OCL-DEPLOY-EXEC-PAUSE-IRREVERSIBLE)

12. **Manage auth yourself.** Run `az login` (interactive device/browser — the operator completes the handshake) and `az account set --subscription <target>`, then **verify** the active subscription is the intended one. If it is wrong or ambiguous, **STOP and surface it** — do not guess the subscription (it is a landmine).
13. **Preflight — dry-run before any apply.** Validate the IaC (`terraform validate` / `az bicep build`), then run the **dry-run** (`terraform plan` / `az deployment ... what-if`) and show the operator the diff. **No apply may run without a prior dry-run.** (OCL-DEPLOY-EXEC-DRYRUN-BEFORE-APPLY)
14. **Execute the irreversible core behind ONE confirm.** Run the autonomous steps (RG/ACR create, image build & push) silently, then **STOP for one blocking confirmation — "apply this plan?"** — covering the infra apply, the Key Vault secret writes, and the pre-traffic migration job (all shown by the dry-run diff). On approval, in order: `terraform apply` / `az deployment create`; provision secrets (`az keyvault secret set`); run the migration job (it must complete **before** any traffic); deploy the new revision at **0% traffic** and health-check it, so a bad rollout cannot displace the running revision. (If the operator prefers, confirm secrets / migration separately — but never run them unconfirmed.)
15. **Go-live gate — never auto-confirmed.** Before routing live traffic, **surface the full production billing multi-factor gate state** (`ENABLE_PRODUCTION_BILLING`, `APP_ENV`, `DATABASE_MARKER`, `PUBLIC_HOST=api.atlas.codes`, `APPLE_ENVIRONMENT`, App Attest prod identity, …) and **STOP for an explicit go-live confirmation**. This is the one stop full-auto never skips; the billing gate is never auto-flipped. On confirmation, route 100% traffic to the new revision. (OCL-DEPLOY-EXEC-GOLIVE-CONFIRMED)
16. **Verify and report.** Confirm ingress responds and the revision is healthy; report what was deployed and that it is live — or, if execution halted, the failing step and its output. Execution honors every `../atlas-ate-server` invariant from step 10 (Key-Vault-only secrets, App Attest, the billing gate, migration-before-traffic, TLS, non-root, port 8080) identically for either dialect.

## Secure-by-default, purpose, access-completeness, landmines & execution safety

These obligations strengthen what the skill does across both phases (full rationale and worked examples in `.mdd/docs/deploy-profile.md`):

- **IaC dialect confirmation is a mandatory blocking pause** — Bicep XOR Terraform, one per run, explicitly confirmed before any deploy artifact is written (step 4). No silent default.
- **Deployment-purpose confirmation is a mandatory blocking pause** — dev / staging / prod, confirmed before any purpose-driven default (step 5). No silent default; purpose *recommends* sizing and posture, the operator confirms.
- **Access-completeness is a mandatory pass** — every secured store needs a named reader AND writer, each with a network path the chosen posture admits, before the diagram is "done" (step 6). A read-but-never-written store, or a writer locked out by your own hardening, is surfaced — never shipped as a complete-looking diagram.
- **Secure-by-default IaC** — the most restrictive network posture consistent with the connectivity the runbook actually requires, runtime AND provisioning (step 8), identically for whichever dialect was confirmed; no store may be left more openly reachable than its peers. Hardening that creates a new requirement (a locked vault needs a writer path) surfaces the doorway; it never bakes one in and never silently relaxes.
- **Generalized landmine detection is a mandatory blocking pause** — any choice whose correctness depends on a fact NOT grounded in the inputs is a go-live landmine, across the app-config, purpose, and access-path axes alike (step 3). Surface it as a blocking question; never demote it to a buried runbook STOP note.
- **Execution safety is mandatory** — a dry-run precedes every apply (step 13); execution halts on the first failed step; only irreversible steps are confirmation-gated while idempotent steps run autonomously (step 14); the go-live cutover is never auto-confirmed and always surfaces the full billing gate state (step 15); the new revision deploys at 0% traffic until health-verified. These replace "never execute" as the safety story.

Caveat: "migrations before traffic" is still enforced procedurally — the executor runs an ordered pre-traffic migration job before routing traffic, behind the apply confirmation — not as an infrastructure interlock. That is a documented, accepted v1 tradeoff — surfaced in the profile, not auto-changed here.

The skill now **executes** the deployment — managing auth, applying, provisioning, and routing traffic to live — instead of stopping at a runbook. The safety that once came from "never execute" now comes from the execution-safety obligations above.

Deploy execution is not a gate.
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

`/mdd-validate` runs the deterministic gate `mdd validate` (engine: `Project::validate()`; `--json` emits a slim `{ok, errors, warnings}` object) rather than re-deriving the checks by hand. It is independent of `mdd review` — it never runs the parity gate. The command walks both `.mdd/models/current/` and `.mdd/models/objective/` and checks:

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

const MDD_RALPH_SKILL: &str = r#"# MDD Ralph

You are an MDD, UML, PlantUML, and OCL specialist running the **Ralph loop** over this repo.

This is a **utility skill, NOT a workflow gate** — a sibling of `/mdd-render` and `/mdd-deploy`. It does not open a cycle, does not gate `/mdd-validate`, `/mdd-implement`, `/mdd-review`, or the `/mdd-cycle` parity loop, and nothing reads a Ralph-specific state tree. It is launched on demand to grind a backlog to completion unattended.

Ralph (after Geoffrey Huntley's technique) is, at its core, `while :; do cat PROMPT.md | agent; done`: each iteration the agent picks **the single most important unfinished thing**, does exactly that one thing, validates, commits, and loops. Here the loop driver is the native **`/loop`** skill, the per-iteration prompt is `.mdd/ralph/PROMPT.md`, and the action vocabulary is **the whole MDD toolbox plus general tools**.

## How to launch

1. **Confirm the plan pointer.** Ralph consumes a plan file but never owns its source — *anything* can write it (the objective-vs-current model gap, a hand-written backlog, an issue export, another agent). Default pointer: `.mdd/ralph/PLAN.md`. Ask the user which plan path to point Ralph at if not the default.
2. **Confirm the branch.** Ralph is unattended and commits each loop. Ralph is a greenfield-leaning technique applied to a mature repo, so **run it on a feature branch, never on `main`.** If the current branch is `main`, create one first.
3. **Launch via `/loop`, self-paced**, feeding `.mdd/ralph/PROMPT.md` as the recurring input with the resolved `$PLAN_PATH`. The loop runs each iteration to completion, then re-fires itself.
4. **Exit** when the prompt emits `RALPH-DONE` (plan has no unfinished items) — stop the loop.

## The contract the loop must honor (do not relax)

- **One item per iteration.** Pick the single highest-priority unfinished item from the plan. Do not batch. This is context-window discipline — degradation is real well before the advertised window.
- **Route, don't hardcode.** Choose the action that best advances the chosen item: `/mdd-cycle` for a feature/change from a description; `/mdd-map` when diagrams drifted from code; `/mdd-generate` when the objective is wrong/missing; `/mdd-implement` then `/mdd-map` for a targeted code gap with intent already agreed; `/mdd-render` or `/mdd-deploy` for inspection/infra items; general tools (Read, Grep, Bash, Edit) for everything else.
- **The parity gate is the only backpressure — it is mandatory.** Full-unattended + free tool choice means a bare `/mdd-implement` or raw `Edit` could drift the models from the code with nothing to catch it. So: **no iteration commits until `/mdd-validate` AND `/mdd-review` pass.** If the iteration used `/mdd-cycle`, that gate already ran; otherwise run it explicitly before committing. Never loosen `.mdd/config.yml` to `warn` to get past it.
- **Don't assume — search first.** Before implementing, search `current/` and the code so you don't re-build something that exists. Fan search/read out to `Explore`/`Agent` subagents; keep build/validate serialized (Ralph's "many search subagents, one build subagent") to protect the main context.
- **Full unattended for modeling/implementation — but irreversible actions still stop.** Resolve ordinary modeling/implementation ambiguity yourself and keep going (a deliberate exception to the standing clarify-before-deciding rule; the parity gate is the safety net that makes it acceptable). This does **not** extend to irreversible or outward-facing actions: a `/mdd-deploy` apply / provision / migration or the **never-auto-confirmed** go-live cutover, force-pushes, deletes, and publishing to external services still pause for explicit human confirmation. The parity gate is backpressure for code; a human is still the gate for go-live.
- **Update the plan and commit every iteration.** Check off the done item, append any bugs discovered mid-flight (even if unrelated), commit with a clear message. A bad unattended iteration is recoverable: abort the cycle / `git reset --hard` on the branch.
- **Self-tune.** When you learn a better build/run/validate command, record it in `AGENTS.md` / `CLAUDE.md` so later iterations inherit it.

## Stop conditions

- Plan has no unfinished items -> emit `RALPH-DONE`, stop.
- The parity gate cannot be made to pass for the chosen item after a reasonable attempt -> record the blocker in the plan, skip the item, continue. If every remaining item is blocked -> emit `RALPH-DONE` with the blocked list, stop.
"#;

const RALPH_PROMPT: &str = r#"# Ralph loop — per-iteration prompt

You are running one iteration of the **Ralph loop** for this repo. The plan pointer for this run is `$PLAN_PATH` (default: `.mdd/ralph/PLAN.md`). Read `.claude/skills/mdd-ralph/SKILL.md` for the full contract; this file is the per-loop instruction.

Do **exactly one** iteration, then return so the loop can re-fire.

## This iteration

1. **Read the plan** at `$PLAN_PATH` and the objective models under `.mdd/models/objective/`. If the plan has no unfinished items, emit `RALPH-DONE` and stop the loop.
2. **Pick the single highest-priority unfinished item.** One item. No batching.
3. **Search before assuming.** Verify it isn't already implemented — search `current/` and the code, fanning reads out to `Explore`/`Agent` subagents. Keep build/validate serialized.
4. **Route to the right action** for that one item:
   - feature/change from a description -> `/mdd-cycle "<item>"`
   - diagrams drifted from code -> `/mdd-map`, then re-plan
   - objective spec wrong/missing -> `/mdd-generate`
   - targeted code gap, intent already agreed -> `/mdd-implement` then `/mdd-map`
   - inspection / infra -> `/mdd-render` or `/mdd-deploy`
   - anything else -> general tools (Read, Grep, Bash, Edit)
5. **Pass the parity gate — mandatory before commit.** If you did not run `/mdd-cycle` (which gates internally), run `/mdd-validate` and `/mdd-review` now and make them pass. Do not commit on a gate failure; do not loosen `.mdd/config.yml`.
6. **Update the plan and commit.** Check off the completed item in `$PLAN_PATH`, append any bugs found mid-flight, and commit with a clear message. If the gate can't be made to pass after a reasonable attempt, record the blocker, skip the item, and continue. If every remaining item is blocked, emit `RALPH-DONE` with the blocked list and stop.
7. **Self-tune** if you learned a better build/run/validate command: record it in `AGENTS.md` / `CLAUDE.md`.

## Rules (do not break)

- Full unattended for **modeling and implementation** decisions: resolve ordinary ambiguity yourself, never pause for it. The parity gate is the safety net.
- **BUT still stop for irreversible / outward-facing actions.** Full-unattended never overrides these — anything hard to undo or that publishes externally needs explicit human confirmation: a `/mdd-deploy` apply / provision / migration / go-live cutover (the go-live gate is **never** auto-confirmed), force-pushes, deletes, or sending to an external service. Pause and ask for these even though everything else runs unattended.
- Never commit to `main`; this loop runs on a feature branch.
- One item per iteration — context discipline.
"#;

const RALPH_PLAN: &str = r#"# Ralph plan

> **Contract.** This is the plan pointer Ralph consumes — the default `$PLAN_PATH`.
> *Anything* may write it: the objective-vs-current model gap, a hand-written backlog,
> an issue-tracker export, or another agent. Ralph **only consumes and updates** it —
> it never owns the source. Point Ralph at a different file by passing `$PLAN_PATH`.
>
> **Format.** A priority-ordered checklist. Highest priority first. Ralph takes the
> single topmost unfinished `- [ ]` item each iteration, completes it through the
> parity gate, then checks it off `- [x]`. Bugs found mid-flight get appended as new
> unfinished items. When no unfinished items remain, Ralph emits `RALPH-DONE`.

## Items

<!-- Replace with real backlog items, highest priority first. Examples: -->
<!-- - [ ] <feature/change described well enough for /mdd-generate or /mdd-cycle> -->
<!-- - [ ] <drift fix: "re-map <area>, diagrams lag the code"> -->

(no items yet — populate before launching Ralph)

## Blocked

<!-- Ralph moves items here, with a one-line reason, when the parity gate can't be made to pass. -->
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

Utility skills (on demand, not a workflow gate):

- `/mdd-render` — render PlantUML diagrams to SVG for external visual inspection.
- `/mdd-deploy` — plan then EXECUTE an Azure Container Apps deployment: generates a UML deployment diagram, runbook, and Bicep or Terraform IaC (operator-confirmed dialect + purpose), then runs the runbook to live traffic — managing auth, dry-running, applying, provisioning, migrating, and routing traffic, pausing only at irreversible steps and a never-auto-confirmed go-live gate, halting on first error; outside the parity gate.
- `/mdd-ralph` — run the Ralph loop: an unattended, self-paced loop (driven by `/loop`) that takes one highest-priority item per iteration from a plan pointer (`.mdd/ralph/PLAN.md` by default; any source may write it), routes it to the right MDD skill or general tools, and must pass the parity gate before committing. Runs on a feature branch, never `main`; exits on `RALPH-DONE`; outside the parity gate.

## Session start: diagrams-first

At the start of a session, run `mdd context` — a Claude Code SessionStart hook (wired by `mdd init` into `.claude/settings.json`) runs it for you and injects the result. It prints a compact whole-map table of contents plus a freshness verdict. Read in that order:

- **FRESH** → trust the diagrams: read the relevant concept diagrams under `.mdd/models/current/`, follow `.mdd/trace.yml` `source_links` to the code, then act.
- **STALE** → the code drifted from the diagrams: run `/mdd-map` on the drifted area FIRST to re-derive `current/`, then read the refreshed diagrams.

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
- `/mdd-deploy` — utility: plan then EXECUTE an Azure Container Apps deployment via a UML deployment diagram, runbook, and generated Bicep or Terraform IaC (operator-confirmed dialect + purpose); runs the runbook to live traffic, pausing only at irreversible steps + a never-auto-confirmed go-live gate and halting on first error; outside the parity gate.
- `/mdd-ralph` — utility: run the Ralph loop — an unattended, self-paced loop (driven by `/loop`) that takes one highest-priority item per iteration from a plan pointer (`.mdd/ralph/PLAN.md` by default; any source may write it), routes it to the right MDD skill or general tools, and must pass the parity gate before committing; runs on a feature branch, exits on `RALPH-DONE`; outside the parity gate.

## Session start: diagrams-first

At the start of a session, run `mdd context`. It prints a compact whole-map table of contents plus a freshness verdict. Codex and other agents cannot auto-fire a session hook, so **run `mdd context` yourself at the start of each session**. Then read in that order:

- **FRESH** → trust the diagrams: read the relevant concept diagrams under `.mdd/models/current/`, follow `.mdd/trace.yml` `source_links` to the code, then act.
- **STALE** → the code drifted from the diagrams: run `/mdd-map` on the drifted area FIRST to re-derive `current/`, then read the refreshed diagrams.

Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative planning context. Validate IDs, refs, and trace links before implementation; report missing rendering, approval, or acceptance-test readiness as warnings instead of blocking implementation.
"#;

const MDD_SECURITY_PROFILE_DOC: &str = include_str!("../../../.mdd/docs/security-profile.md");
const MDD_DEPLOY_PROFILE_DOC: &str = include_str!("../../../.mdd/docs/deploy-profile.md");
