# Test Profile

The test profile records, **per layer**, which runner verifies *this* repo and
how it is invoked. It is the test analog of `deploy-profile.md`: the runner is
**detected from the build files, then operator-confirmed** — there is no silent
default. It lives in two places:

- `.mdd/config.yml` — the `test:` block (machine-readable; consumed by the gate).
- this doc — the human-readable contract and the detection table.

## What Cycle A ships (structure only)

This is the first phase (docs/proposals/tdd-and-test-assertion.md §9, Cycle A).
It adds **structure**, not execution:

- A unified `tests` collection in `.mdd/trace.yml`, with `layer`, `framework`,
  and `expect` per link. Legacy `generated_tests` / `generated_ui_tests` keep
  working and are projected into the same view.
- Structural `mdd validate` rules: test files exist, model IDs resolve, and —
  **only when `test.layers` is configured** — layer/framework membership and
  per-kind coverage. With no layers configured (the default) these rules are
  **inert**, so a repo that has not adopted diagram-driven tests is unaffected.

Detection/confirmation of the runner, `mdd test-plan`, the close-time green
gate, and the non-negotiable red→green evidence gate are **later cycles**.

## The `test:` config block

```yaml
# .mdd/config.yml
test:
  gate: error          # error (default) | warn — coverage-rule severity
                       # (later: also the close-time green gate). NOTE: the
                       # red->green requirement is non-negotiable and is NOT a
                       # config knob; `gate` governs the green side only.
  layers:              # per-layer profile — EMPTY until detect-then-confirm
                       # (a later cycle) populates it. Empty => rules inert.
    # unit:
    #   framework: cargo-test
    #   command: "cargo test --workspace --lib"
    # ui:
    #   framework: playwright
    #   command: "npm --prefix mockups run test:ui"
```

`gate` reuses the `error | warn` pattern of `security.parity_check` /
`traceability.parity_check`. While `layers` is empty, the coverage and
membership rules do not fire — they have no profile to judge against.

## Layers (the diagram-kind → layer taxonomy)

| Diagram kind | Layer | Typical ID prefix |
| --- | --- | --- |
| `domain/` + OCL | `unit` | `UT-` |
| `sequences/`, `components/` | `integration` | `IT-` |
| `use-cases/` | `e2e` / `acceptance` | `AT-` |
| `mockups/` | `ui` | `UIT-` |
| `@sec(...)` markers | `security` | `SECT-` |

A test link declares which layer it occupies so the coverage rule can match it
to the kind that expects it.

## Detection table (for the later detect-then-confirm cycle)

Detection *recommends*; the operator *confirms*. A polyglot/monorepo (this repo:
Rust crates + a TS/React `mockups/` app) is the normal case, so the profile is
per-layer and a layer may target a subdirectory. Ambiguity is surfaced as a
blocking question, never guessed.

| Build file | Unit / integration | E2E / UI |
| --- | --- | --- |
| `Cargo.toml` | `cargo test` / `cargo nextest run` | — |
| `package.json` | `vitest` / `jest` | `playwright` / `cypress` |
| `pyproject.toml` / `setup.py` | `pytest` | `playwright` / `pytest-bdd` |
| `go.mod` | `go test ./...` | — |
| `pom.xml` / `build.gradle` | `mvn test` / `gradle test` | Selenium / Playwright-java |
| `*.csproj` | `dotnet test` | Playwright-.NET |
| `Gemfile` | `rspec` | Capybara |
