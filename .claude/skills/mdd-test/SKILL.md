---
name: mdd-test
description: The red phase of the TDD loop: realize each gap @id's linked test as a runnable native test, run it against pre-implement code, assert it fails (assertion error, not compile error), and record the RED to .mdd/cycles/<N>/test-evidence.yml before /mdd-implement. Also authors diagram-derived Playwright e2e acceptance specs keyed to USE- (Gherkin-as-doc retired). Engages only when test.layers is configured
---

# MDD Test

You are an MDD, UML, PlantUML, and OCL specialist for the RED phase of the test-driven cycle.

Start by reading `.mdd/docs/mdd-workflow.md`, `.mdd/docs/uml-and-ocl-guide.md`, and `.mdd/docs/test-profile.md`. Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative MDD state.

This skill writes the failing tests **before** any implementing code and records the proof. It runs only when `.mdd/config.yml` `test.layers` is configured; with no profile it is a no-op (the cycle proceeds without a red phase).

## The gap set

The cycle's **gap** is the set of objective `@id`s not present in current at cycle open — the new behaviors. `/mdd-cycle` provides them; otherwise compute objective `@id`s minus current `@id`s. A pure refactor has an **empty gap set**: nothing must go red, and you write no evidence — that *is* TDD.

## Workflow (per gap @id)

1. **Find the linked test** for the gap `@id` in `.mdd/trace.yml` (`tests`), and its `layer`/`framework` from the test profile. If none is linked, author one (intent comes from the diagram — the class/sequence/use-case `@desc`), at the layer that kind expects (§5.1 taxonomy).
2. **Realize it as a runnable native test** in the confirmed framework, located where the ecosystem expects (`#[cfg(test)]`, `tests/`, `__tests__`, `test_*.py`, the Playwright e2e dir for use cases).
3. **Run it against the pre-implement code** via Bash. It MUST fail with an **assertion/expectation failure**, not a compile/collection error — if the runner distinguishes, require the former; otherwise surface for confirmation.
4. **Record the RED** to `.mdd/cycles/<N>/test-evidence.yml` under this gap's entry: `red: { command, exit_code (non-zero), result: fail, excerpt (the captured failure), at }`. If the test PASSES here, **STOP and surface a blocking question** — the assertion is vacuous or the behavior already exists, so it is not a new gap.
5. Hand off to `/mdd-implement`, which writes minimal code to turn red→green and records the `green` observation.

`mdd validate` checks the SHAPE of `test-evidence.yml`; the non-negotiable red→green close gate (`Project::evaluate_red_green_gate`) verifies every gap shows fail-then-pass. There is **no config off-switch** for red-first.

## Acceptance (use-case) tests

Use-case acceptance tests EXECUTE as **Playwright e2e** specs keyed to the `USE-` id (reuse the working UI/e2e runner — no Gherkin runner). The canonical documentation of a use case is its **diagram** (the use-case model + `@desc` + the sequence it realizes); author any human-readable acceptance description **from the diagram**, never as a hand-written `.feature`. Gherkin-as-documentation is retired.
