# Design Proposal — `/mdd-kickoff`: Greenfield Project Kickoff → Ralph

**Status:** reviewed; 5 decisions locked · **Date:** 2026-05-25 · **Author:** design session
**Decision owner:** repo maintainer · **Delivery:** phased MDD cycles (later session)

## 1. Summary

Today MDD has a strong *back half* for building a system — `/mdd-generate`
turns a description into the objective model, `/mdd-cycle` drives one slice to
parity, and `/mdd-ralph` grinds a `PLAN.md` checklist to `RALPH-DONE`
unattended. It has **no front half**: nothing interviews a developer about a
*new* project, aligns on objective and architecture, and produces the
populated `PLAN.md` that the back half consumes. `mdd init` deliberately seeds
an **empty** `.mdd/ralph/PLAN.md` and leaves the question of who fills it open
(`lib.rs:873-892`: "a plan that the model gap, a backlog, or another agent now
owns").

This proposal adds **`/mdd-kickoff`** — a new interactive front-door skill (a
utility sibling of `/mdd-ralph` / `/mdd-deploy`, **outside the parity gate**)
that, for a greenfield repo:

1. **Interviews** the developer to full alignment on the objective ("what's the
   final outcome of the build") and the architecture — clarification-heavy,
   never guessing (the standing clarify-before-deciding rule, made the explicit
   contract of this phase).
2. Writes a **signed-off project brief** (`.mdd/docs/brief.md`): objective,
   in/out-of-scope, architecture decisions (ADR-lite), and the best-practice
   tooling choices for the chosen stack.
3. Runs `/mdd-generate` on the agreed brief to produce the **full objective
   model** — *all* the diagrams — then `/mdd-validate` to gate it clean.
4. **Decomposes** that objective model into a priority-ordered `.mdd/ralph/PLAN.md`,
   foundations-first, where each model-bearing item declares the objective
   `@id` subset it realizes.
5. **Stops** with a kickoff summary and the exact next command. It does *not*
   launch Ralph — the human reviews the brief + PLAN, then runs `/mdd-ralph`.

The one piece of genuinely new *core* machinery is **scoped parity**: because
the complete objective exists from day one, the whole-model parity gate
(`Project::review()`, `lib.rs:1485`) would mark every not-yet-built id as a
mismatch and **no per-item Ralph cycle could ever close** until the entire
project is done. Each PLAN item therefore carries its `@id` scope; the parity
gate closes a cycle against *that subset*; whole-model parity becomes the
natural `RALPH-DONE` condition. Everything else reuses existing machinery (the
`@id`/`@ref`/`@cycle` marker convention, the cycle manifest + snapshot tree,
the `detect-then-confirm` discipline from `/mdd-deploy` and `mdd test-detect`,
and the SeededOnce `PLAN.md` hook that was always meant for "another agent").

## 2. Current state (grounded)

| Concern | Today | Where |
| --- | --- | --- |
| New-project intake | **None.** No skill interviews the developer or aligns on objective/architecture before modeling | — |
| Objective from a description | `/mdd-generate` derives objective diagrams; clarifies only *minimally* ("mark the ambiguity… ask for review"), does **not** own an interview | `mdd-generate/SKILL.md:14-50` |
| PLAN.md authorship | `mdd init` seeds an **empty** `PLAN.md` (SeededOnce, `DOM-INIT-SEED-ONCE`); explicitly "owned by the model gap, a backlog, or another agent" later | `lib.rs:873-892`, `templates.rs:91-93` |
| Ralph consumption | Picks the topmost unfinished `- [ ]`, routes it, must pass the parity gate before commit, `RALPH-DONE` when none remain | `mdd-ralph/SKILL.md:21-34` |
| Parity gate | `Project::review()` is **whole-model**: `missing = objective_ids − current_ids`, `ids_matched = missing.is_empty()` | `lib.rs:1501-1505` |
| Cycle entry selection | description → `/mdd-generate`; no description → `/mdd-map`-only-stop. **No "realize an existing objective slice" entry** | `mdd-cycle/SKILL.md:18-21` |
| Cycle boundary | manifest `{number, slug, entry, description, status, opened_at, touched_files}`; no scope field | `mdd-cycle/SKILL.md:27-37` |
| Architecture decisions | Captured only implicitly in component diagrams; no decision-record doc | — |

**Three gaps** follow directly:

1. **Intake** — there is no place to *discover* a greenfield project: no
   interview, no alignment gate, no decision record.
2. **Planning** — nothing turns an objective model into the priority-ordered,
   appropriately-sized PLAN that Ralph needs; a human must hand-write it.
3. **Per-item parity** — the gate is whole-model, so it cannot validate the
   incremental progress of a plan that is realizing one complete-up-front
   objective slice at a time.

## 3. Goals / non-goals

**Goals**

- A single skill takes a developer from "I want to build X" to a **validated
  objective model + a Ralph-ready PLAN**, with every objective-alignment and
  architecture question resolved *before* any code or plan is produced.
- The discovery phase **never guesses** a genuinely ambiguous decision — it
  pauses and asks, and ends at an explicit **brief sign-off gate**.
- Best-practice tooling for the chosen stack is **recorded** (as brief decisions
  and as PLAN items); Ralph *sets it up* through the gate — kickoff itself does
  not scaffold tooling (locked §4).
- The PLAN is **derived from the objective model**: kickoff runs `/mdd-generate`
  and decomposes the result; every model element is a gap, since greenfield.
- Ralph can drive that complete-up-front objective to completion **one item per
  iteration**, each item passing a *scoped* parity gate, with whole-model
  parity reached exactly when the plan is exhausted.

**Non-goals**

- Not an auto-pilot: kickoff **stops** at a populated PLAN; a human launches
  Ralph (locked §4). No auto-`/loop`.
- Kickoff does **not** scaffold CI/linters/formatters itself (locked §4) — it
  records them as PLAN items.
- `Project::review()` stays a **structural** gate. Scoped parity narrows *which
  ids* it checks; it does not add execution or new semantics beyond a filter.
- Not a replacement for `/mdd-cycle` on a mature repo: kickoff is the
  greenfield front door; incremental change still goes through `/mdd-cycle`.
- Not brownfield onboarding (mapping an existing codebase into MDD) — that is
  `/mdd-map`'s job and is out of scope here.

## 4. Decisions locked in this session

| Axis | Decision |
| --- | --- |
| **Form factor** | A **new `/mdd-kickoff` skill** — a utility front door outside the parity gate. `mdd-cycle` / `mdd-generate` / `mdd init` are left untouched (init still seeds the empty PLAN; kickoff populates it). |
| **Tooling scope** | Best-practice tooling is **recorded as brief decisions + PLAN items**; **Ralph sets it up** through the gate. Kickoff does not write CI/lint/format/test config itself. |
| **PLAN source** | **Derived from the objective model.** Kickoff runs `/mdd-generate` on the agreed brief to produce the *full* objective, then decomposes it. The PLAN is grounded in `@id`s, not free prose. |
| **Handoff** | Kickoff **stops** after writing PLAN + a summary. The human reviews and launches `/mdd-ralph` (on a feature branch) + `/loop` when ready. No auto-launch. |
| **Parity reconciliation** | **Scoped parity.** Full objective lives in `.mdd/models/objective/` from kickoff. Each PLAN item declares the `@id` subset it realizes; the gate closes a cycle against *that subset*; whole-model parity = `RALPH-DONE`. (The "objective grows per item / zero gate change" alternative was weighed and **not** adopted — it softens the full-objective-up-front choice.) |

## 5. Design

### 5.1 The kickoff flow

`/mdd-kickoff` is a utility skill (sibling of `/mdd-ralph`, `/mdd-deploy`,
`/mdd-render`) — it does **not** open or close a cycle, does not gate the
parity loop, and writes no `.mdd/cycles/<N>/` tree. Its phases:

```
/mdd-kickoff [one-line idea]
  0. PREFLIGHT   confirm greenfield + workspace initialized
  1. INTERVIEW   elicit objective + architecture to full alignment (clarify, never guess)
  2. BRIEF       write .mdd/docs/brief.md → STOP at the sign-off gate (human confirms)
  3. GENERATE    /mdd-generate on the agreed brief → full objective model
  4. VALIDATE    /mdd-validate → objective is structurally clean
  5. DECOMPOSE   derive priority-ordered .mdd/ralph/PLAN.md (foundations-first, @scope per item)
  6. SUMMARY     report brief path, objective counts, PLAN items, the next command → STOP
```

Two human gates: **brief sign-off** (step 2, before any model is generated) and
**the stop** (step 6, before Ralph runs). Everything between is mechanical.

**Preflight (step 0).** Kickoff is for greenfield. Detect it: `.mdd/models/objective/`
and `.mdd/models/current/` empty, no closed cycle under `.mdd/cycles/`, and
`PLAN.md` still the seed. If the repo is *not* greenfield, **surface it as a
blocking question** ("objective model already exists / PLAN already populated —
kickoff will operate alongside it; or use `/mdd-cycle` for incremental change")
rather than hard-blocking or clobbering. If `.mdd/` is absent, offer to run
`mdd init` first.

### 5.2 The brief (`.mdd/docs/brief.md`) — the alignment artifact

A single human-readable document, the operational sibling of
`deploy-profile.md` / `test-profile.md` / `security-profile.md`. It is the
**spec `/mdd-generate` consumes** and the thing the developer signs off, so it
is the durable record of "what we agreed to build and why." Sections:

- **Objective** — the product vision and the *final outcome* of the build (the
  whole-project definition of done).
- **In scope** — the capabilities/use cases this build delivers.
- **Out of scope / non-goals** — explicitly bounded, to keep the objective
  model and PLAN from sprawling.
- **Architecture decisions** (ADR-lite) — one entry per significant choice:
  *context → decision → rationale → alternatives considered*. Stack, framework,
  data store, API style, key integrations, deployment target, major component
  boundaries.
- **Tooling** — the best-practice toolchain for the chosen stack (formatter,
  linter, test framework(s), CI, pre-commit, docs). These become PLAN items
  (§5.5), not kickoff actions.
- **Constraints / NFRs** — scale, performance, security/compliance/PII,
  must-use technologies, timeline.

ADR-lite-in-one-file is the default (the "prefer simpler" reading). Separate
`.mdd/docs/decisions/NNNN-*.md` ADR files are a noted alternative if the
maintainer wants them.

### 5.3 The interview — clarify, never guess

The interview is the heart of the requirement ("get the full picture… all
questions about objective alignment and architecture must be clarified"). It is
an iterative chat (free-form + `AskUserQuestion` for discrete choices) covering:
objective/outcome; actors & primary flows; architecture (stack, framework, data,
integrations, deploy target, component boundaries); NFRs (scale, security, PII,
availability); constraints (must-use tech, existing systems); and the toolchain.

**The rule is the contract of this phase, not an exception to it:** whenever a
decision is genuinely ambiguous, **stop and ask** — kickoff inherits
`/mdd-cycle`'s "clarification is mandatory" discipline (`mdd-cycle/SKILL.md:14-16`)
and the repo's standing clarify-before-deciding rule. The phase **ends only at
the brief sign-off gate**: kickoff presents `brief.md` and does not proceed to
generate until the developer confirms it. Tooling recommendations reuse the
**detect-then-confirm** pattern (`mdd test-detect` from the test profile, the
`/mdd-deploy` dialect rule): kickoff *recommends* a stack-idiomatic toolchain
and the developer *confirms* — no silent default.

### 5.4 Full objective up front + the parity collision

Step 3 generates the **complete** objective model (every diagram kind), so
after kickoff `.mdd/models/objective/` is full and `.mdd/models/current/` is
empty — every objective `@id` is a gap.

That collides head-on with the parity gate Ralph must pass *before every
commit*. `Project::review()` is whole-model (`lib.rs:1501-1505`):

```rust
let missing: BTreeSet<String> = objective_ids.difference(&current_ids).cloned().collect();
let ids_matched = missing.is_empty();
```

With the full objective present, after Ralph builds item 1, items 2…N are still
`missing` → `ids_matched == false` → the cycle cannot close. Ralph's
one-item-per-iteration contract (`mdd-ralph/SKILL.md:21-25`) breaks: no
intermediate item could ever commit. **Scoped parity is the resolution.**

### 5.5 PLAN.md schema — `@scope`, two item kinds, foundations-first

`PLAN.md` stays a human-readable, priority-ordered checklist (Ralph's contract:
topmost unfinished `- [ ]` each iteration). Kickoff adds, on each *model-bearing*
item, an inline **`@scope(<id>, <id>, …)`** marker naming the objective `@id`s
that item realizes — reusing the repo-wide `@id`/`@ref`/`@cycle` marker idiom,
so the line stays readable and parses deterministically.

```markdown
## Items

<!-- Foundations first: infra items (no @scope) establish the build + test harness -->
- [ ] Scaffold the project: build system, entrypoint, source layout, README
- [ ] Wire the toolchain: formatter, linter, test runner, CI pipeline, pre-commit

<!-- Then core domain, then features in dependency order. @scope = objective ids realized -->
- [ ] Realize the Task domain model + invariants @scope(DOM-TASK, OCL-TASK-TITLE-LEN)
- [ ] Realize user authentication (login + RBAC) @scope(USE-LOGIN, SEQ-LOGIN, CMP-AUTH, SEC-LOGIN)
- [ ] Realize create/list/complete task flows @scope(USE-TASK-CRUD, SEQ-TASK-CREATE, CMP-TASK-API)
- [ ] Realize the task list UI @scope(MCK-TASK-LIST, UIC-TASK-ROW)

## Blocked
```

**Two item kinds:**

- **Model-bearing** (carries `@scope`): Ralph routes to `/mdd-cycle` in its new
  *realize-slice* entry (§5.6) — no new objective is generated; the cycle drives
  current to the scoped objective and closes on scoped parity.
- **Infra / tooling** (no `@scope`): Ralph routes to general tools (the "general
  tools for everything else" branch, `mdd-ralph/SKILL.md:24`). These touch no
  model ids, so parity is trivially unchanged; the item commits after
  `/mdd-validate` passes. The first infra item (project scaffold) must come
  first so later cycles have a build, a test harness, and code to `/mdd-map`.

**Ordering** (kickoff's responsibility): (1) scaffold + toolchain (infra),
(2) core domain + invariants, (3) features/use-cases in dependency order,
(4) cross-cutting/security, (5) optional `/mdd-deploy` handoff item. Each
model-bearing item is sized to **one cycle** — roughly one use case plus its
directly-supporting sequence/components, or one cohesive domain cluster.

**Coverage invariant.** The union of all `@scope` id sets must cover every
implementation-bearing objective `@id`. This is what guarantees that "Ralph
exhausts the PLAN" ≡ "whole-model parity is reached" — so `RALPH-DONE` (plan
empty, `mdd-ralph/SKILL.md:33`) coincides with `missing.is_empty()`. Kickoff
self-checks it; `mdd validate` enforces it (§5.8, `OCL-KICKOFF-PLAN-COVERS-OBJECTIVE`).

### 5.6 Scoped review (the core change)

Three small, contained changes turn the whole-model gate into a scope-aware one.
Absent a scope, behavior is **identical to today** (back-compatible).

1. **`Project::review()` gains an optional scope** (`lib.rs:1485`). A
   `review_scoped(scope: Option<&BTreeSet<String>>)` (or a `scope` param):
   when `Some`, `missing` is intersected with `scope` before
   `ids_matched = missing.is_empty()`; ids outside the scope that are still
   missing are **expected**, not failures. Security parity (`review_security`,
   `lib.rs:1576`) filters to markers hosted on scope ids the same way. When
   `None`, the existing whole-model path runs unchanged. The diff-puml emission
   (`lib.rs:1507-1553`) annotates only the in-scope gap.

2. **The cycle manifest gains `scope: [ids]`** (optional). Present → the cycle
   is scoped; absent → whole-model (today's `/mdd-cycle`). The `Open` step
   (`mdd-cycle/SKILL.md:27-37`) records it; `mdd-review` reads it.

3. **`/mdd-cycle` gains a *realize-slice* entry.** Today: description → generate;
   no-description → map-only (`mdd-cycle/SKILL.md:18-21`). New third mode: invoked
   with a scope (the PLAN item's `@scope` ids) and **no new description to
   model**, it skips `/mdd-generate`, opens a cycle with `scope` in the manifest,
   and loops `/mdd-validate → (/mdd-test red, if test.layers) → /mdd-implement →
   /mdd-map → /mdd-validate → /mdd-review(scoped)` until scoped parity. The
   objective is read-only here — it was authored once, by kickoff.

This is deliberately a **general** capability ("review/close against just this
feature's ids"), useful well beyond greenfield; kickoff is its first consumer.

### 5.7 Ralph routing

Ralph needs one rule added to its routing table (`mdd-ralph/SKILL.md:24`): an
item with `@scope(...)` → `/mdd-cycle` *realize-slice* with that scope; an item
without → general tools (infra), commit after `/mdd-validate`. Everything else
about Ralph is unchanged — one item per iteration, parity gate before commit
(now possibly scoped), `RALPH-DONE` when the plan empties, feature-branch-only,
self-paced `/loop`. The TDD red→green gate (when `test.layers` is configured)
rides along inside the realize-slice cycle, scoped to the item's gap ids — kickoff
can pre-author the per-layer test intent during `/mdd-generate` (it already does,
`mdd-generate/SKILL.md:47`).

### 5.8 CLI / core changes

- **`Project::review()` / `review_security()`** — optional scope filter (§5.6).
  Pure structural narrowing; fully unit-testable (mirror the existing
  `review_reports_*` tests at `lib.rs:5566+` with scoped variants).
- **`mdd validate`** gains structural rules (existence/shape only):
  - every PLAN `@scope(...)` id exists in the objective registry;
  - PLAN coverage: the union of `@scope` ids ⊇ the implementation-bearing
    objective id set (WARNING by default; the kickoff self-check is the
    authoritative producer).
- **(Optional) `mdd plan-coverage --json`** — a deterministic verb that reports
  the objective ids, the PLAN `@scope` union, and the uncovered/unknown sets
  (the planning analog of `mdd test-plan`). Minimal fallback: kickoff and
  validate compute coverage inline without a new verb.

### 5.9 Skill changes

| Skill | Change |
| --- | --- |
| **`/mdd-kickoff`** *(new)* | The whole flow §5.1: preflight → interview (clarify, never guess) → brief + sign-off gate → `/mdd-generate` full objective → `/mdd-validate` → decompose into a foundations-first `PLAN.md` with `@scope` → stop + summary. Utility skill, outside the parity gate. |
| `/mdd-cycle` | New *realize-slice* entry (§5.6): scope-driven, skips generate, records `scope` in the manifest, loops to **scoped** parity. |
| `/mdd-review` | Honor the manifest `scope`: in-scope `missing` only; out-of-scope gaps are expected. Doc the whole-model default is unchanged. |
| `/mdd-ralph` | One routing rule: `@scope` item → realize-slice cycle; infra item → general tools (§5.7). |
| `/mdd-generate` | No behavior change; documents that kickoff is a caller that hands it the agreed brief as the description. |

### 5.10 New invariants

Add `.mdd/constraints/kickoff.ocl` with `OCL-KICKOFF-*` invariants (mirrors
`deploy-iac.ocl` / `test-assertion.ocl`):

- `OCL-KICKOFF-SCOPE-IDS-EXIST` — every PLAN `@scope` id resolves to an
  objective `@id`.
- `OCL-KICKOFF-PLAN-COVERS-OBJECTIVE` — the `@scope` union covers every
  implementation-bearing objective id (so `RALPH-DONE` ≡ whole parity).
- `OCL-KICKOFF-BRIEF-SIGNED-OFF` — the objective model is generated only after
  `brief.md` exists and is confirmed (no model before alignment).
- `OCL-REVIEW-SCOPE-SUBSET-OF-OBJECTIVE` — a cycle's manifest `scope`, when
  present, is a subset of the objective registry.

## 6. Worked example (greenfield "team task tracker")

```
/mdd-kickoff "a small team task-tracker API"
```

1. **Preflight** — empty objective + current, PLAN still seed → greenfield ✓.
2. **Interview** — kickoff elicits: actors (member, admin); capabilities
   (auth, CRUD tasks, assign, complete); architecture (Rust + axum + Postgres,
   REST, JWT auth); NFRs (RBAC, input length limits, rate limit on login);
   toolchain (`cargo fmt`, `clippy`, `cargo test`/`nextest`, GitHub Actions).
   Ambiguity ("multi-tenant?") → **paused and asked**; answer: single-tenant v1.
3. **Brief** — `.mdd/docs/brief.md` written with objective, in/out scope, ADR-lite
   (axum chosen over actix: *rationale + alternatives*), tooling, NFRs. Developer
   **signs off**.
4. **Generate + validate** — `/mdd-generate` produces `USE-LOGIN`, `USE-TASK-CRUD`,
   `SEQ-*`, `DOM-TASK`, `DOM-USER`, `CMP-AUTH`, `CMP-TASK-API`, `OCL-TASK-TITLE-LEN`,
   `MCK-TASK-LIST`, `@sec(...)` markers (`<<ByPassing>>` on login, `<<Flooding>>`
   rate limit, `<<BufferOverflow>>` on title); `/mdd-validate` clean.
5. **Decompose** — `PLAN.md` (foundations-first), e.g.:
   ```
   - [ ] Scaffold the axum project: workspace, main, router, README
   - [ ] Toolchain: rustfmt, clippy, cargo-nextest, GitHub Actions CI
   - [ ] Realize User + Task domain + invariants @scope(DOM-USER, DOM-TASK, OCL-TASK-TITLE-LEN)
   - [ ] Realize auth + RBAC @scope(USE-LOGIN, SEQ-LOGIN, CMP-AUTH, SEC-LOGIN)
   - [ ] Realize task CRUD @scope(USE-TASK-CRUD, SEQ-TASK-CREATE, CMP-TASK-API)
   - [ ] Realize task list UI @scope(MCK-TASK-LIST, UIC-TASK-ROW)
   ```
   `@scope` union covers every impl-bearing id (`OCL-KICKOFF-PLAN-COVERS-OBJECTIVE` ✓).
6. **Summary + stop** — "objective: 6 use-case ids, 4 components, 2 domain
   classes, 1 mockup; PLAN: 6 items (2 infra, 4 scoped). Review `brief.md` and
   `PLAN.md`, then run `/mdd-ralph` on a feature branch + `/loop`."

Then **Ralph** (separate, human-launched): item 1 (infra) scaffolds the project
→ validate → commit. Item 3 (`@scope(DOM-USER, DOM-TASK, …)`) → realize-slice
cycle: implement those classes → map → **scoped** review (only those ids must
match; `USE-LOGIN` etc. still missing is *expected*) → close. …Item 6 closes →
current == full objective → whole parity → plan empty → `RALPH-DONE`.

## 7. Backward compatibility & migration

- **Scoped review is opt-in by a scope.** No manifest `scope` ⇒ `review()` runs
  the existing whole-model path verbatim. Every current `/mdd-cycle`,
  `/mdd-review`, and Ralph drift-fix item is unaffected.
- **`PLAN.md` stays SeededOnce.** `mdd init` still seeds the empty template
  (`lib.rs:887-892`); kickoff *writes* the file directly as a skill (not via the
  init conflict handler), which is exactly the "another agent owns it" path the
  SeededOnce comment describes (`lib.rs:873-878`). On re-run with a
  *non-empty* PLAN, kickoff **confirms before overwriting** (no clobber).
- **`@scope` is additive** — a plain `- [ ]` line with no `@scope` is a valid
  infra item; existing hand-written PLANs keep working.
- **`mdd init` gains** a `brief.md` template scaffold + `kickoff.ocl`, under the
  established **state-file safety** rule (never overwrite trace/config/approvals;
  `--force` overwrites regenerable docs; forward-only migration — cycle 0018).
- The new `mdd validate` rules default to **WARNING**, so a repo without a PLAN
  or without `@scope` markers does not start failing.

## 8. Risks & open questions

1. **Interview completeness / stop condition.** "Full picture" is subjective.
   *Mitigation:* the brief is the artifact and the **sign-off gate** is explicit
   — the developer, not kickoff, decides it is complete. A short "open questions"
   section in the brief surfaces anything deferred.
2. **PLAN item sizing.** Too coarse → a realize-slice cycle can't close in one
   iteration; too fine → churn. *Mitigation:* the "one use case + its supporting
   sequence/components, or one domain cluster" heuristic; Ralph already appends
   discovered work and can split a stuck item (record blocker, re-scope).
3. **Infra items escape the parity gate** (no `@scope`, touch no model ids).
   This is by design (locked §4: tooling is recorded, Ralph sets it up), but
   their correctness is not model-checked. *Mitigation:* they are still gated by
   `/mdd-validate` + commit, and the brief records the intended toolchain.
4. **`@scope` drift.** A scope id could name a non-existent objective id, or the
   union could miss an objective id (Ralph would `RALPH-DONE` before parity).
   *Mitigation:* `OCL-KICKOFF-SCOPE-IDS-EXIST` + `OCL-KICKOFF-PLAN-COVERS-OBJECTIVE`,
   enforced by `mdd validate`.
5. **Whole-objective up front can be wrong up front.** Generating the entire
   model before any code risks over-design or early mistakes. *Mitigation:* the
   brief sign-off; the objective is still refinable — a Ralph item may re-`/mdd-generate`
   a slice if the design proves wrong mid-flight (and re-scope the PLAN), the
   same recovery `/mdd-cycle` already allows.
6. **Greenfield detection false-negatives** (a repo that is "mostly" empty).
   *Mitigation:* detection **surfaces a question**, never hard-blocks or
   clobbers (§5.1).

## 9. Phased delivery (≈2–3 cycles)

- **Cycle A — scoped parity core.** `Project::review()`/`review_security()`
  optional scope filter; cycle manifest `scope` field; `/mdd-cycle`
  realize-slice entry; `/mdd-review` scope honoring; `@scope(...)` parsing +
  `mdd validate` scope-ids-exist rule; `OCL-REVIEW-SCOPE-SUBSET-OF-OBJECTIVE`.
  Generally useful beyond kickoff; **lands independently** with no behavior
  change when no scope is present. *Pure machinery — safe.*
- **Cycle B — the kickoff skill.** `/mdd-kickoff` SKILL.md (interview → brief +
  sign-off → generate → validate → decompose → stop); `brief.md` template;
  Ralph routing rule (`@scope` vs infra); PLAN coverage invariant +
  `mdd validate` coverage rule; embed the skill in `templates.rs` `WORKFLOW_SKILLS`
  (`templates.rs:7-58`); `mdd init` brief scaffold; `kickoff.ocl`; doc updates
  (`mdd-workflow.md`, `CLAUDE.md` skill list).
- **(Optional) Cycle C — polish.** Greenfield detection + re-run-safety UX;
  `detect-then-confirm` toolchain recommendation reusing `mdd test-detect`;
  `/mdd-deploy` handoff as the trailing PLAN item; `mdd plan-coverage --json`.

## 10. File-by-file change map (for the implementer)

| File | Change |
| --- | --- |
| `crates/mdd-core/src/lib.rs:1485` (`review`) | Optional scope filter on `missing`/`ids_matched`; scoped diff-puml emission |
| `crates/mdd-core/src/lib.rs:1576` (`review_security`) | Same scope filter on security markers (by host id) |
| `crates/mdd-core/src/lib.rs` (validate) | `@scope` ids exist; PLAN coverage rule; severity from config (WARNING default) |
| `crates/mdd-core/src/lib.rs` (tests `5566+`) | Scoped variants of `review_reports_*` |
| `crates/mdd-cli` | *(optional)* `mdd plan-coverage --json` |
| `.mdd/cycles/<N>/manifest.yml` | New optional `scope: [ids]` field |
| `.mdd/ralph/PLAN.md` (schema) | Inline `@scope(<id>,…)` on model-bearing items; infra items have none |
| `.mdd/docs/brief.md` | New alignment artifact (objective, scope, ADR-lite, tooling, NFRs) |
| `.mdd/constraints/kickoff.ocl` | `OCL-KICKOFF-*` + `OCL-REVIEW-SCOPE-*` invariants |
| `crates/mdd-core/src/templates.rs:7-58` | Add `MDD_KICKOFF_SKILL` to `WORKFLOW_SKILLS`; `brief.md` template; init scaffolds it |
| `.claude/skills/mdd-kickoff/SKILL.md` | New skill (§5.1, §5.9) |
| `.claude/skills/mdd-{cycle,review,ralph}/SKILL.md` | Per §5.6–5.7 |
| `.mdd/docs/mdd-workflow.md`, `CLAUDE.md` | Document the greenfield front door + scoped parity |
