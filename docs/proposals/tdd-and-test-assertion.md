# Design Proposal — Diagram-Driven Tests, the Test-Profile, and a TDD Cycle Gate

**Status:** reviewed; Cycle-A blockers locked · **Date:** 2026-05-25 · **Author:** design session
**Decision owner:** repo maintainer · **Delivery:** phased MDD cycles (later session)

## 1. Summary

Today MDD *scaffolds* two narrow kinds of test (acceptance Gherkin keyed to use
cases, Playwright keyed to mockups), checks only that the files **exist and are
linked**, and **never runs them**. The parity gate ignores tests entirely.

This proposal makes MDD **assert tests for real**, across all diagram kinds,
using **the best runner for the app's actual stack**, and reshapes `/mdd-cycle`
into a **test-driven loop**: diagrams → failing tests derived from the diagrams →
code that makes them pass → parity. A cycle **physically cannot close** until
every new behavior has a recorded **red→green** transition and the linked suite
is green. **Red-first is non-negotiable** — an enforced, evidence-backed close
gate, not a config knob.

It reuses existing machinery wherever possible (the unused `category` field on
the trace model, the `framework` field, the `parity_check: error|warn` config
pattern, the per-cycle artifact + snapshot machinery under `.mdd/cycles/<N>/`,
and the detect-then-confirm discipline already proven in `/mdd-deploy`).

## 2. Current state (grounded)

| Concern | Today | Where |
| --- | --- | --- |
| Test authoring | `/mdd-generate` writes acceptance `.feature` (`AT-<USE>`) + Playwright `.spec.ts` (`UIT-<MCK>`) + security (`SECT-<SEC>`) **scaffolds** and links them | `mdd-generate/SKILL.md` steps 4, 9 |
| Trace model | `generated_tests[{id,path,model_id,category?}]`, `generated_ui_tests[{id,path,model_id,framework="playwright"}]`, `source_links` | `crates/mdd-core/src/lib.rs:157-201` |
| Validate — mockups | **ERROR**: an implementation-ready mockup (route + element) with no linked Playwright test | `lib.rs:2344-2350` |
| Validate — files | **ERROR**: any linked test file missing on disk | `lib.rs:2309-2336` |
| Validate — acceptance | **WARNING only**: a use case with no acceptance test; `framework` value is *not* validated (non-`playwright` silently dropped) | `lib.rs:1771-1781`, `2338-2341` |
| Review (cycle gate) | ID parity + security parity + traceability parity — **zero test logic** | `lib.rs:993-1082` |
| Execution | **Nothing runs tests.** UI specs run by hand via `npm --prefix mockups run test:ui` (Playwright config points at `.mdd/tests/ui`); the Gherkin `.feature` files have **no runner at all** | `mockups/playwright.config.ts`, `mockups/package.json` |
| Implement | Writes app code only; never authors or runs tests | `mdd-implement/SKILL.md` |
| Tool selection | `"playwright"` hardcoded (`default_ui_test_framework`); no language/build detection anywhere | `lib.rs:3018-3020` |

**Three gaps** follow directly:

1. **Coverage** — only use-cases and mockups map to tests. Domain classes, OCL
   invariants, sequences, components, and state machines map to nothing.
2. **Tooling** — only Playwright/Gherkin exist; no detection of `cargo test`,
   `pytest`, `vitest`/`jest`, `go test`, JUnit, etc.
3. **Assertion** — the gate checks *existence*, never *execution*. A stub passes.

## 3. Goals / non-goals

**Goals**

- Every implementation-bearing diagram element traces to at least one real,
  runnable test of the appropriate **layer**.
- The test runner per layer is **detected from the app's build files, then
  operator-confirmed** — no silent default, no hardcoded framework.
- `/mdd-cycle` is **test-driven and red-first is non-negotiable**: for each new
  behavior a test is written from the diagrams, **observed failing and that
  failure recorded before code**, then code makes it green, and the recorded
  red→green transition is required to close — with no config off-switch.
- Backward compatible: existing `AT-`/`UIT-`/`SECT-` links keep working.

**Non-goals**

- Not a coverage-percentage gate (no line/branch thresholds in v1).
- `Project::review()` stays a **structural** parity gate — it does not run tests.
  Test execution and the red→green evidence gate live in the `/mdd-cycle`
  close step, mirroring how `/mdd-deploy` executes outside the parity gate.
- Not auto-selecting a runner without confirmation (polyglot/monorepo guess-wrong
  risk); detect *recommends*, the operator *decides* — the `/mdd-deploy` rule.

## 4. Decisions locked in this session

| Axis | Decision |
| --- | --- |
| Assertion strength | **Run + gate on green** at cycle close, green gate configurable `error`/`warn` |
| TDD | **Red-first is NON-NEGOTIABLE** — no off-switch. Enforced by a recorded red→green transition per new behavior (§5.4); diagrams → failing tests → code to green → loop |
| Tool selection | **Detect, then confirm** into a test-profile (mirrors `/mdd-deploy` dialect) |
| Coverage | **All four layers**: unit (domain + OCL), integration (sequences/components), e2e/acceptance (use cases), UI (mockups) |
| Delivery | **Design doc first** (this document); implement later as phased cycles |
| Trace schema | **Unified `tests` array** (§5.2): new collection with `layer`/`framework`/`expect`; legacy `generated_tests`/`generated_ui_tests` project into it. The extend-in-place fallback is **rejected** — resolves §8.5 |
| Red evidence content | **Runner output + exit code** (§5.4): `test-evidence.yml` records the actual `command`, `exit_code`, and a captured failure `excerpt` per phase — not a bare `result` boolean. Harder to fabricate; resolves §8.4. (A pre-implement git ref was considered and **not** adopted — output+exit code judged sufficient.) |
| Red gate off-switch | **None** — confirmed. Red→green is non-negotiable with no config and no operator override; a hard-to-red behavior is surfaced for fixing, never waved through. Only `test.gate` (green side) is configurable |

Two distinct knobs, do not conflate them:

- **Red-first** — *non-negotiable*, always enforced, no config. Every gap test
  must be recorded failing before its code lands (§5.4).
- **Green gate** (`test.gate: error|warn`) — does a *still-red* test at close
  block closure (`error`, default) or merely warn and allow user-accepted close
  (`warn`)? This is the opt-down, mirroring `security.parity_check`.

## 5. Design

### 5.1 Test taxonomy — diagram → layer → test

The join is **diagram kind → test layer**. Each layer has an ID prefix, a
default home, and a trace relation.

| Diagram kind | Layer | New/used ID prefix | Lives where (default) | Trace relation |
| --- | --- | --- | --- | --- |
| `domain/` (`DOM-`) + OCL (`OCL-`) | **unit / property** | `UT-` *(new)* | app-native unit location (`#[cfg(test)]`, `tests/`, `__tests__`, `test_*.py`) | `verified_by` |
| `sequences/` (`SEQ-`), `components/` (`CMP-`) | **integration** | `IT-` *(new)* | app-native integration location | `verified_by` |
| `use-cases/` (`USE-`) | **e2e / acceptance** | `AT-` *(reuse)* | `.mdd/tests/acceptance/` or app e2e dir | `verified_by` (today: `generated_tests`) |
| `mockups/` (`MCK-`) | **UI** | `UIT-` *(reuse)* | `.mdd/tests/ui/` | today: `generated_ui_tests` (unchanged) |
| `states/` (`STM-`) | **transition** (a unit/integration flavor) | reuse `UT-`/`IT-` | with the domain class's tests | `verified_by` |
| `@sec(...)` (`SEC-`) | **security** | `SECT-` *(reuse)* | with the layer it guards | today: generated from markers |

Principle: **tests live where the app's ecosystem expects them**, and trace
*links* point there. `.mdd/tests/` stays the home only for framework-agnostic
specs that have no natural app home (Gherkin acceptance, the Playwright
UI-contract specs). Forcing Rust unit tests into `.mdd/tests/` would fight the
toolchain; we link to `crates/*/src/**` instead.

### 5.2 Trace schema (minimal, back-compatible)

Reuse the latent `category` field and add `layer`/`framework`/`expect`. No struct
is removed; deserialization of existing files is unchanged.

```yaml
version: 1
tests:                       # NEW unified collection (preferred)
  - id: UT-DOM-USER          # unit test for a domain class
    path: crates/mdd-core/src/domain/user.rs
    model_id: DOM-USER
    layer: unit              # unit | integration | e2e | acceptance | ui | security
    framework: cargo-test    # resolved from the test-profile
    expect: pass             # pass (default) | red-until-implemented (gap marker)
  - id: IT-SEQ-LOGIN
    path: crates/mdd-cli/tests/login_flow.rs
    model_id: SEQ-LOGIN
    layer: integration
    framework: cargo-test
generated_tests:             # KEPT: acceptance, back-compatible (category => layer)
  - id: AT-USE-LOGIN
    path: .mdd/tests/acceptance/use-login.feature
    model_id: USE-LOGIN
    category: acceptance
generated_ui_tests:          # KEPT verbatim: UI layer already works
  - id: UIT-MCK-LOGIN-FORM
    path: .mdd/tests/ui/mck-login-form.spec.ts
    model_id: MCK-LOGIN-FORM
    framework: playwright
source_links: [...]          # unchanged
```

Migration reads `generated_tests`/`generated_ui_tests` and projects them into the
unified `tests` view internally; new links are written to `tests`. **Locked
decision (§4):** the unified `tests` array is the data model; the extend-in-place
variant is rejected. This is the schema Cycle A builds on, so it is settled before
Cycle A starts, not deferred.

### 5.3 The test-profile — detect, then confirm

A new operational doc `.mdd/docs/test-profile.md` (sibling of
`deploy-profile.md` / `security-profile.md`) plus a `test:` block in
`.mdd/config.yml`. The profile records, **per layer**, the runner command for
*this* repo, resolved by detection and **operator-confirmed** — the same blocking
discipline `/mdd-deploy` uses for its IaC dialect (no silent default).

```yaml
# .mdd/config.yml
test:
  gate: error               # error (default) | warn — does a STILL-RED test block close?
  # NOTE: red-first is NON-NEGOTIABLE and is NOT a config knob. Every gap test
  # must be recorded failing before its code lands (§5.4). There is no off-switch
  # — `gate` only governs whether a test still red AT CLOSE blocks or warns.
  layers:
    unit:
      framework: cargo-test
      command: "cargo test --workspace --lib"
    integration:
      framework: cargo-test
      command: "cargo test --workspace --test '*'"
    e2e:
      framework: playwright
      command: "npm --prefix mockups run test:ui"
    ui:
      framework: playwright
      command: "npm --prefix mockups run test:ui"
    acceptance:
      framework: cucumber-rs   # or "unwired" — see risk in §8
      command: "cargo test --test acceptance"
```

**Detection table** (build-file → recommended runner; detect *recommends*, the
operator confirms):

| Build file | Unit / integration | E2E / UI |
| --- | --- | --- |
| `Cargo.toml` | `cargo test` (or `cargo nextest run`) | — |
| `package.json` | `vitest` / `jest` (from devDeps) | `playwright` / `cypress` (from devDeps) |
| `pyproject.toml` / `setup.py` | `pytest` | `playwright`/`pytest-bdd` |
| `go.mod` | `go test ./...` | — |
| `pom.xml` / `build.gradle` | `mvn test` / `gradle test` (JUnit) | Selenium/Playwright-java |
| `*.csproj` | `dotnet test` | Playwright-.NET |
| `Gemfile` | `rspec` | Capybara |

Polyglot/monorepo (this very repo: Rust crates + a TS/React `mockups/` app) is
the **normal** case, so the profile is **per-layer**, and a layer may target a
subdirectory. Detection that is ambiguous (two unit frameworks, no clear home) is
**surfaced as a blocking question**, never guessed — the `/mdd-deploy` landmine
rule applied to test tooling.

### 5.4 The TDD cycle (the core reshape) — red-first, enforced

Today's loop is `validate → implement → map → validate → review`. The
test-driven loop inserts a **recorded red phase** before code and a **red→green +
green gate** at close:

```
ENTRY (description) -> /mdd-generate  (objective diagrams + per-layer test intent/links)
   -> /mdd-validate                   (structural: every impl-bearing @id has a linked test)
   -> RED:   realize the gap's tests as runnable native tests; RUN them against the
             pre-implement code; RECORD the FAIL to .mdd/cycles/<N>/test-evidence.yml.
             A gap test that PASSES here BLOCKS (vacuous, or behavior already exists).
   -> GREEN: /mdd-implement writes minimal code; RUN tests until pass; RECORD the PASS
   -> /mdd-map -> /mdd-validate       (refresh current; structural gate incl. test-trace)
   -> /mdd-review                     (ID + security + traceability parity — structural)
   -> CLOSE GATE:  parity matched
               AND every gap test shows red->green in test-evidence.yml   (NON-NEGOTIABLE)
               AND suite green per test.gate                              (error blocks / warn accepts)
        | all satisfied -> DONE
        | otherwise     -> loop
```

**The "gap"** is the set of objective `@id`s not yet present in `current` at
cycle open. These are the cycle's *new behaviors*; each must have ≥1 linked test
that goes red→green.

**Red evidence — the non-negotiable proof.** "The test failed before the code
existed" is a claim about *history*; by close the test is green, so without a
record there is nothing to verify. The red phase therefore **records the
observation** as a first-class cycle artifact, written before `/mdd-implement`
runs and completed by the green step:

Each phase records not a bare `result` boolean but the **actual runner evidence**
— the `command` run, its `exit_code`, and a captured failure/pass `excerpt` of the
runner output (locked decision §4). A pass/fail boolean alone is trivially
fabricated by the same agent that writes the code; the captured command + exit
code + output excerpt is the runner's own artifact and is far harder to invent
without actually running it. (A pre-implement git ref was weighed and not adopted
— output + exit code is sufficient anchoring for v1.)

```yaml
# .mdd/cycles/<N>/test-evidence.yml   (red phase writes `red`; green step writes `green`)
version: 1
cycle: 0023
gap_tests:
  - id: UT-DOM-USER
    model_id: DOM-USER                  # objective @id absent from current at open
    layer: unit
    red:                                # observed BEFORE /mdd-implement
      command: "cargo test --lib rejects_bad_email"
      exit_code: 101                    # non-zero => failed, as required
      result: fail
      excerpt: |                        # captured runner output (assertion failure, not compile error)
        test rejects_bad_email ... FAILED
        assertion failed: User::new("nope").is_err()
      at: "2026-05-25T18:30:00Z"
    green:                              # observed AFTER implement + map
      command: "cargo test --lib rejects_bad_email"
      exit_code: 0
      result: pass
      excerpt: "test rejects_bad_email ... ok"
      at: "2026-05-25T18:41:00Z"
```

`mdd` validates the **shape** (both phases present with `command`/`exit_code`/
`result`/`excerpt`) and that `red.exit_code != 0` while `green.exit_code == 0`;
the §8.4 assertion-vs-compile-error check inspects the `red.excerpt`.

**Close requires every `gap_tests` entry to show `red.result: fail` then
`green.result: pass`.** This requirement has **no config switch** — it is the
literal "tests from diagrams that do not pass, then code to pass the test" made
mechanical. A gap test recorded `red.result: pass` is surfaced as a blocking
question (vacuous assertion, or the behavior already exists — so it is not new)
and must be fixed or explicitly justified by the operator; it is never silently
accepted.

**No-gap cycles are correct without a red test.** A pure refactor (no new
objective `@id`) has an empty `gap_tests`; nothing must go red — that *is* TDD —
and only the green gate (the full suite stays green) applies. So "non-negotiable"
means *every new behavior proves red→green*, not *every cycle invents a failure*.

**Refactor (third TDD beat).** After green, code cleanup is allowed only while
the suite stays green; `/mdd-map` then re-derives `current/` from the cleaned
code.

### 5.5 Where the gates live

`Project::review()` stays **purely structural** (ID + security + traceability).
Running arbitrary project suites is non-deterministic, slow, and environment-
dependent — a bad fit for the unit-tested deterministic core, and inconsistent
with `/mdd-deploy`, whose *execution* is deliberately **not** a CLI verb.

Recommended split (mirrors deploy's "plan is deterministic, execution is
agent-Bash"):

- **`mdd test-plan --json`** *(new, deterministic)* — resolves `config.yml` `test`
  + the trace links into an ordered **plan**: `[{layer, id, model_id, command,
  cwd, expect}]`, with the gap subset flagged. This is the test analog of the
  deploy *runbook*. Pure data; no execution; unit-testable.
- **`/mdd-cycle` executes the plan via Bash** — runs the gap subset at the red
  phase and writes `test-evidence.yml`; runs the full suite at the green step and
  completes the evidence; enforces the close gate. Halts and reports on failure,
  exactly like the deploy executor walks its runbook.

The **close gate** (in the `/mdd-cycle` Close step, not in `review()`) is the
conjunction of three checks:

1. **Parity** — `Project::review()` matched (structural).
2. **Red→green evidence** — every `gap_tests` entry in `test-evidence.yml` shows
   `fail` then `pass`. **Non-negotiable**; no config disables it.
3. **Green** — the full linked suite is green, governed by `test.gate`
   (`error` blocks close on any remaining red; `warn` reports and allows a
   user-accepted close — the opt-down, like `security.parity_check`).

`mdd` validates the **shape** of `test-evidence.yml` deterministically (schema,
that every gap `@id` has an entry); the **running** that produces the
fail/pass results is agent-Bash, same plan/execute split as deploy.

Minimal fallback: no `test-plan` verb; the skill reads `config.yml` `test.layers`
and runs the commands directly, still writing `test-evidence.yml`. Loses the
deterministic, testable plan artifact but keeps the non-negotiable red→green gate.

### 5.6 CLI changes (`mdd validate` / new `mdd test-plan`)

`mdd validate` gains **structural** rules (existence/linkage/shape only — still no
execution):

- Every implementation-bearing `@id` (domain, sequence, component, use case,
  mockup; configurable per kind) has ≥1 linked test of the expected layer →
  ERROR if `test.gate: error`, else WARNING.
- Every linked test path exists on disk (already true for `generated_*`; extend
  to `tests`).
- `framework`/`layer` values are members of the configured `test.layers` set
  (replaces today's silent drop of non-`playwright`).
- `.mdd/config.yml` `test` block is well-formed when present.
- `test-evidence.yml`, when present in a cycle dir, is schema-valid.

`mdd test-plan` (new) emits the ordered execution plan as JSON (§5.5).

### 5.7 Skill changes

| Skill | Change |
| --- | --- |
| `/mdd-generate` | Author per-layer test **intent + links** for every impl-bearing `@id` (not just use cases/mockups). Mark gap tests `expect: red-until-implemented`. |
| **`/mdd-test`** *(new, recommended)* | The **red phase**: realize the gap's linked tests as runnable native tests in the confirmed framework, run them against pre-implement code, **assert red and write `test-evidence.yml`**, then hand to implement. *(Alternative: fold into `/mdd-implement`'s first sub-phase — see §8.)* |
| `/mdd-implement` | The **green phase**: write minimal code to turn red → green; keep the suite green; record the green observation; no longer "app code only." |
| `/mdd-cycle` | Resolve+confirm the test-profile on first run (detect → block → confirm); run `mdd test-plan` and execute it; **enforce the non-negotiable red→green close gate** + the `test.gate` green gate. |
| `/mdd-validate` | Surface the new structural test rules + `test-evidence.yml` shape check. |
| `/mdd-map` | When mapping existing code, discover native tests and refresh test links so brownfield repos start with real coverage mapped. |
| `/mdd-review` | Unchanged (structural parity only); doc note that the red→green and green gates are sibling Close-step gates, not review passes. |

### 5.8 New invariants

Add `.mdd/constraints/test-assertion.ocl` with `OCL-TEST-*` invariants (mirrors
`deploy-iac.ocl`'s `OCL-DEPLOY-*`), e.g.:

- `OCL-TEST-EVERY-IMPL-ID-HAS-TEST` — each impl-bearing `@id` has ≥1 linked test.
- `OCL-TEST-PROFILE-CONFIRMED` — a layer's `framework` is confirmed before its
  tests are authored (no silent default).
- `OCL-TEST-RED-EVIDENCE-RECORDED` — *(non-negotiable)* every gap `@id` has a
  `gap_tests` entry in `.mdd/cycles/<N>/test-evidence.yml` whose `red.result` was
  observed `fail` before its implementing code; evidence source is the artifact,
  not a heuristic.
- `OCL-TEST-RED-THEN-GREEN-TO-CLOSE` — *(non-negotiable)* the cycle closes only
  when every gap test shows `fail` then `pass`. No config disables this.
- `OCL-TEST-GREEN-GATE` — a still-red test at close blocks when `test.gate: error`
  (warns when `warn`).
- `OCL-TEST-LAYER-IN-PROFILE` — every link's `layer`/`framework` is in `config`.

## 6. Worked example (this repo, polyglot)

Cycle: "add an email-format invariant to `User`."

1. **Diagrams** — `/mdd-generate` adds `DOM-USER.email` + `OCL-USER-EMAIL-FORMAT`,
   and links `UT-DOM-USER` (layer `unit`, framework `cargo-test`, `expect:
   red-until-implemented`). `DOM-USER` is a gap `@id` (absent from current).
2. **Validate** — passes structurally; `UT-DOM-USER` linked.
3. **Red** — `/mdd-test` writes `#[test] fn rejects_bad_email()` in
   `crates/mdd-core/src/domain/user.rs`, runs `cargo test --lib rejects_bad_email`
   → **fails**, and records `red: { result: fail }` for `UT-DOM-USER` in
   `.mdd/cycles/<N>/test-evidence.yml`. (Had it passed here, the cycle blocks.)
4. **Green** — `/mdd-implement` adds the email check; `cargo test` → **passes**;
   records `green: { result: pass }`.
5. **Map + validate + review** — `current/` re-derived; parity matches.
6. **Close gate** — parity ✓; `UT-DOM-USER` shows `fail→pass` ✓ (non-negotiable);
   full suite (`cargo test` + `npm --prefix mockups run test:ui`) green ✓ →
   cycle closes.

A use-case change would additionally drive an `AT-`/e2e test; a mockup change the
existing Playwright `UIT-` path (unchanged).

## 7. Backward compatibility & migration

- Existing `generated_tests` / `generated_ui_tests` keep deserializing; they
  project into the unified `tests` view (`category`→`layer`, default
  `framework: playwright` for UI).
- Repos with no `test` config block: `mdd validate`'s coverage rule is a
  **WARNING** (opt-in via `test.gate: error`). The **red→green close gate is part
  of `/mdd-cycle`**, so it engages once tests are linked; a cycle with no gap
  tests (empty `gap_tests`) closes as today — existing projects don't break.
- `mdd init` gains: scaffold `.mdd/docs/test-profile.md`, write a `test` block to
  `config.yml`, add `test-assertion.ocl`. Respect the established **state-file
  safety** rule (never overwrite trace/config/approvals; `--force` overwrites
  regenerable docs; forward-only migration) — same machinery as cycle 0018.

## 8. Risks & open questions

1. **Acceptance `.feature` has no runner today.** Making `AT-` "runnable" means
   either wiring a Gherkin runner (`cucumber-rs`, `pytest-bdd`, `cucumber-js`) or
   treating use-case coverage as Playwright/integration e2e. **Decision needed**
   in the implementation cycle. Cheapest first step: map use-case e2e onto the
   already-working Playwright path and leave Gherkin as documentation.
2. **Red phase: dedicated `/mdd-test` skill vs. fold into `/mdd-implement`.** A
   dedicated skill keeps "write failing test" and "write code" cleanly separated
   (true TDD) and gives the red→green evidence a clear owner; folding it in is
   less surface. **Recommend** a dedicated skill; confirm at implementation time.
3. **Running suites in the loop** costs time and can be flaky/environment-bound.
   Mitigations: per-layer scoping, run only the gap's tests in the red-check, full
   suite only at the green gate, and `test.gate: warn` as a green-side escape
   hatch (the red→green requirement itself has no escape hatch).
4. **Red-check false-greens — RESOLVED (§4).** The evidence artifact now captures
   the runner's own `command`/`exit_code`/`excerpt`, not a bare boolean, so the
   `red.result: fail` claim is backed by output that is hard to fabricate without
   running. Residual nuance: a test can be red for the *wrong* reason
   (compile/import error vs. a real assertion). v1 mitigation — the red phase
   inspects `red.excerpt` and requires an assertion/expectation failure, not a
   collection/compile error; otherwise it surfaces for confirmation. A pre-implement
   git ref was considered and not adopted (output+exit code sufficient for v1). The
   *existence* of red→green is mechanically gated, not heuristic.
5. **Unified `tests` array vs. extend `generated_tests` — RESOLVED (§4).** Unified
   collection chosen; legacy arrays project into it. The extend-in-place fallback
   is rejected. Settled before Cycle A since it defines that cycle's data model.
7. **No off-switch on red→green — CONFIRMED (§4).** This is a deliberate new
   precedent versus the repo's `error|warn` pattern. No config and no operator
   override; a legitimately hard-to-red behavior (concurrency, time-dependent) is
   surfaced for fixing or re-scoping, never waved through — an override would
   reopen the §8.4 self-attestation hole the evidence artifact just closed. Only
   `test.gate` (the green side) remains configurable.
6. **Sandbox/network** for running app suites (Playwright needs a dev server;
   integration tests may need services). The profile's `command` carries setup
   (`webServer` already does for Playwright); document per-layer preconditions.

## 9. Phased delivery (≈3 cycles)

- **Cycle A — taxonomy + trace + structural validate.** Unified `tests` model,
  `layer`/`framework`, new prefixes, `mdd validate` coverage rule (WARNING by
  default), `test-assertion.ocl`, `test` config block + `mdd init` scaffolding.
  No execution yet. *Pure structure — lands safely.*
- **Cycle B — test-profile + `mdd test-plan` + green gate.** Detection table,
  detect-then-confirm UX in `/mdd-cycle`, the deterministic plan verb, and the
  close-time green gate (`test.gate`). Wire `/mdd-implement` to keep green.
- **Cycle C — TDD red phase (non-negotiable) + full layer coverage.** `/mdd-test`
  (or implement sub-phase) realizes gap tests and writes
  `.mdd/cycles/<N>/test-evidence.yml`; the **non-negotiable red→green close
  gate** (`OCL-TEST-RED-EVIDENCE-RECORDED`, `OCL-TEST-RED-THEN-GREEN-TO-CLOSE`);
  `/mdd-map` test discovery; and the acceptance-runner decision from §8.1.

## 10. File-by-file change map (for the implementer)

| File | Change |
| --- | --- |
| `crates/mdd-core/src/lib.rs:157-201` | Add `tests` collection + `layer`/`framework`/`expect`; project legacy arrays |
| `crates/mdd-core/src/lib.rs` validate | New coverage + layer-membership rules; `test-evidence.yml` shape check; gate severity from `config.test.gate` |
| `crates/mdd-core/src/lib.rs:3018-3020` | Remove the hardcoded `"playwright"` default in favor of profile resolution |
| `crates/mdd-cli` | New `mdd test-plan --json` subcommand |
| `.mdd/cycles/<N>/test-evidence.yml` | New per-cycle artifact: gap-test red→green record (red phase writes `red`, green step writes `green`); the non-negotiable close gate reads it |
| `.mdd/config.yml` | New `test:` block (schema in §5.3); `gate` governs the green side only |
| `.mdd/docs/test-profile.md` | New operational doc (detection table, per-layer contract, the non-negotiable red→green gate, green-gate behavior) |
| `.mdd/constraints/test-assertion.ocl` | New `OCL-TEST-*` invariants incl. the two non-negotiable red→green ones |
| `.claude/skills/mdd-{generate,implement,cycle,validate,map}/SKILL.md` | Per §5.7 |
| `.claude/skills/mdd-test/SKILL.md` | New red-phase skill that writes `test-evidence.yml` (if adopted) |
| `.mdd/docs/mdd-workflow.md`, `CLAUDE.md`, `AGENTS.md` | Document the red-first TDD loop, the red→green + green gates, and the test-profile |
```
