# Assessment — Executable Constraints & `@sec` (mostly already designed/shipped)

**Status:** gap analysis for review · **Date:** 2026-05-27 · **Author:** Ralph (PLAN item 4)
**Decision owner:** repo maintainer · **Delivery:** finish an existing slice, *after* sign-off

> Nothing new is implemented. Per the Ralph "search before assuming" rule, this
> item turned out to be **largely covered by existing work** — so the honest
> deliverable is a gap analysis that defers to that work, not a fresh proposal
> that re-invents it.

## 1. The PLAN item vs. reality

The PLAN item (from the source review) asks: OCL constraints and `@sec` markers
"document rules that nothing enforces at runtime" — back each with a real
assertion/test so the rule is *enforced, not drawn*.

**Grounded finding: this is not greenfield.** It is the explicit subject of a
**reviewed** proposal — `docs/proposals/tdd-and-test-assertion.md` — and the
machinery is **partially shipped** (diagram-driven tests, cycles 0023–0025).

## 2. What already exists (grounded)

| Capability | State | Where |
| --- | --- | --- |
| OCL/domain → tests design | Designed: `domain/`(`DOM-`) + OCL(`OCL-`) map to a **unit/property** layer via a new `UT-` id + `verified_by` relation | `tdd-and-test-assertion.md` §table (lines ~100–105) |
| `@sec` → executable test design | Designed: `@sec`(`SEC-`) → **security** layer, **reuse `SECT-`** ids, run + gate | same proposal (line ~105) |
| Security test scaffolding | **Shipped**: `Project::generate_security_tests()` emits `SECT-<SEC>` per implementation-ready marker + trace entry | `lib.rs:2427`, `:2473`, `security_test_scaffold` `:4539` |
| Security test **coverage gate** | **Shipped**: an implementation-ready `@sec` marker with no linked `SECT-` test is a **validation error** | `lib.rs:3114-3123` |
| `Security` test layer + `category` | **Shipped**: `TestLayer::Security` → `"security"`; `GeneratedTest.category` round-trips `category: security` | `lib.rs:357-375`, `:325`, test `:5531` |
| OCL coverage invariant | **Shipped**: `OCL-TEST-EVERY-IMPL-ID-HAS-TEST` — every implementation-bearing element must have a linked test of its expected layer | `.mdd/constraints/test-assertion.ocl` |
| Red→green + green gate | **Shipped**: non-negotiable red→green evidence + configurable green gate at cycle close | `test-assertion.ocl` `OCL-TEST-RED-*`/`OCL-TEST-GREEN-GATE`; `lib.rs` `evaluate_*_gate` |

So the framework the review asks for — *constraints/markers are verified by real,
executed, gated tests* — is **already designed and largely built**.

## 3. Why it still *looks* inert in this repo

`.mdd/config.yml` has `test.layers: {}` (empty). The entire diagram-driven-test
system is **safe-by-default inert** until layers are configured (see
`test-assertion.ocl`: "enforced only when the profile has configured layers …
with no configured layers it is inert"). So in *this* repo nothing runs — but
that is a **configuration choice, not a missing capability**. The review observed
the symptom (nothing enforced) and inferred a missing mechanism; the mechanism
exists and is simply unconfigured here.

## 4. The genuine remaining gap (narrow)

Two real deltas remain beyond "configure `test.layers`":

1. **OCL → executable unit/property tests is the *unshipped* slice.** The
   `tdd-and-test-assertion.md` table marks the `UT-` id and the `verified_by`
   relation **"(new)"**. Security (`SECT-`) shipped; the OCL/domain unit slice
   appears designed-but-not-built. OCL is not directly executable, so an
   `OCL-` invariant is enforced by a **linked native unit/property test**
   (`context Order inv total>=0` → a `#[test]`/property test asserting it).
2. **`SECT-` scaffolds must be fleshed to real assertions.** `generate_security_tests`
   emits a *scaffold*; the red→green gate (already shipped) is what forces it to
   actually fail-then-pass — but only once the security layer is configured and
   the scaffold is filled. This is execution discipline, not new code.

## 5. Recommendation (simplest path, no new methodology)

Do **not** author a new proposal or a parallel enforcement system. Instead:

- **Close PLAN item 4 by pointing at `tdd-and-test-assertion.md`** as the
  authoritative design; this assessment is the cross-reference.
- **If the maintainer wants OCL actually enforced**, schedule the one unshipped
  slice from that proposal: the `UT-` unit/property layer + `verified_by`
  relation linking each `OCL-`/`DOM-` invariant to a native test, gated by the
  already-built green / red→green machinery. That is a normal `/mdd-cycle`
  feature against the existing design — not a methodology shift.
- **Optionally adopt `test.layers` for this repo** (run `mdd test-detect`,
  confirm the Rust `unit` layer) so the existing coverage + gate machinery stops
  being inert here and the project dogfoods its own assertion story.

## 6. Decisions needed (for sign-off)

1. Accept that item 4 is **mostly already designed/shipped** and close it against
   `tdd-and-test-assertion.md` rather than a new proposal? *(Recommended.)*
2. Schedule the **`UT-` / OCL-unit slice** (the one unshipped piece) as a future
   `/mdd-cycle` feature? Yes / defer.
3. Adopt `test.layers` (unit) for this repo so the machinery runs here? Yes / defer.
