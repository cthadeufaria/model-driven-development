---
name: mdd-review
description: Two-pass cycle-closure: ID parity + security-marker parity (security-by-default error gate); emits .diff.puml / .security.diff.puml on mismatch
---

# MDD Review

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

## Scope (realize-slice cycles)

When the open cycle's manifest declares a non-empty `scope` (a set of objective `@id`s), `Project::review()` narrows **both passes** to that slice automatically: `missing` is intersected with the scope and `@sec` markers are compared only for in-scope hosts, so objective ids and guards **outside** the scope that are absent from current are **expected**, not a mismatch (they belong to other PLAN items / future realize-slice cycles). An empty or absent scope is the whole-model gate — the default for ordinary cycles and a bare `mdd review`. You do **not** pass the scope; review reads it from the highest-numbered open cycle. Whole-model parity is therefore reached exactly when the last slice closes.

## Closure rule

The cycle is **done** only when **ID parity matches AND (security parity matches OR `security.parity_check` is `warn` and the user accepts the listed warnings)** — over the scope in effect (the whole model when no scope is declared). ID parity is strict structural; the security gate is `error` by default. The user may override a result manually (e.g. "ignore this extra"), but the default decision is automatable from `Project::review()`.
