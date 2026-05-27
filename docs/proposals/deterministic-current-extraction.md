# Design Proposal — Deterministic Current-Side Extraction (AST → PlantUML)

**Status:** draft for review · **Date:** 2026-05-27 · **Author:** Ralph (PLAN item 2)
**Decision owner:** repo maintainer · **Delivery:** phased MDD cycles, *after* sign-off
**Depends on / composes with:** `mdd map-scope` (cycle 0030, shipped)

> **This is a methodology shift, not a described feature.** Nothing here is
> implemented. The deliverable of this Ralph iteration is the proposal itself;
> the PLAN item is explicit that no code lands before sign-off.

## 1. Summary

The single biggest AI-token cost in an MDD cycle is `/mdd-map` re-deriving the
`current/` side: an agent reads the source and **hand-writes PlantUML that
mirrors the code**. But `current/` is, by definition, a faithful reflection of
what the code already is — so the *mechanical* parts of that mirror (the classes,
their fields, the module/dependency structure) are deterministically derivable
from the source's AST with **zero tokens**, in milliseconds, and would be
**correct by construction** rather than "accurately transcribed by an AI."

This proposal adds a deterministic extractor — `mdd map-extract` — that emits the
**structurally-derivable** current-side diagrams (domain/class and component)
directly from a language AST + build manifests, and **narrows the agent's job**
to the parts that genuinely need judgment (use-case intent, sequence
ordering/call-graph, and the security/`@desc` semantic overlay). It reuses the
`syn`-based symbol engine the repo already runs for `source_links`
(`crates/mdd-core/src/traceability.rs::extract_symbols`), generalized behind a
language-adapter trait so a guest repo (e.g. Swift via `swift-syntax`/SourceKit)
plugs in without touching the core.

Crucially, **`source_links` become a generation output**: if a class node is
emitted *from* `Foo` in `crates/x/src/foo.rs`, the link is known at emit time, so
the agent stops hand-authoring trace entries for extracted ids.

## 2. Current state (grounded)

| Concern | Today | Where |
| --- | --- | --- |
| Current-side authoring | Agent reads code and **hand-writes** all `current/**.puml` (use-case, domain, component, sequence, state) | `.claude/skills/mdd-map/SKILL.md` steps 2–7 |
| Already-deterministic AST | `extract_symbols` parses `.rs` via `syn::parse_file` + `proc-macro2` span-locations → `SymbolSpan { path, symbol, line_start, line_end, kind }`; indexes enum variants as `Enum::Variant`; skips `#[test]`/`#[cfg(test)]` | `crates/mdd-core/src/traceability.rs:48`, `:31` |
| What the AST reader captures | symbol **name + kind + line span** only — **not** fields, types, relationships, or multiplicities | `traceability.rs` `SymbolSpan` |
| Source-link resolution | `symbol_matches` tolerates bare leaf + `Type::method`; forward parity already maps `@id → (path, symbol)` | `traceability.rs:39`, `lib.rs:review_traceability` |
| Markers carried by each diagram | `@id`, `@ref` (same-side), `@desc` (one-line for the viewer card), `@sec` (security overlay), `@ui-*` (mockups) | `.mdd/docs/uml-and-ocl-guide.md`, `security-profile.md` |
| Scoped re-map (just shipped) | `mdd map-scope --cycle N` returns the affected current diagrams; `/mdd-map` re-derives only those | cycle 0030, `lib.rs::map_scope` |
| Trace authoring | `source_links` (`{model_id, path, symbol}`) are **hand-written** by the agent during `/mdd-map` | `mdd-map/SKILL.md` step 8, `.mdd/trace.yml` |
| Freshness | `mdd map-status` already diffs code vs the whole-map baseline by symbol | `lib.rs::map_status` |

**Reading:** the project already does Rust AST parsing for *one* purpose
(resolving/covering `source_links`). The leap this proposal asks for is to use
that same parse to **emit** the mechanical diagrams, not just to check links —
plus a modest enrichment of the symbol model (fields, types, edges).

## 3. Goals / non-goals

**Goals**

- Eliminate AI tokens from the *mechanical* half of `/mdd-map` (domain/class +
  component structure) by generating it deterministically.
- Keep the agent in charge of the *semantic* half (use cases, sequence
  ordering, `@desc`, `@sec` judgement) where there is no ground truth in the AST.
- Make `source_links` for extracted ids a **generation output**, not hand-authored.
- Stay **language-pluggable**: a `LangAdapter` trait; Rust (`syn`) first, Swift
  (`swift-syntax`/SourceKit) as the guest-repo target.
- **Compose with `map-scope`**, not fight it: extraction is naturally scoped to
  the changed source set.

**Non-goals**

- Generating use-case diagrams (intent is not in the AST — stays agent-authored).
- Generating sequence diagrams from a static call graph in v1 (see §5.3 — ordering
  and alternate/failure flows need judgment; agent-assisted).
- Replacing the `objective/` side (that is derived from a *description*; this
  touches `current/` only).
- Replacing OCL/`@sec` enforcement (that is PLAN item 4's territory).
- A full LSP client (over-engineered for a single-language host; adapter trait
  leaves the door open).

## 4. The extractable / agent-only split (the core analysis)

| Diagram kind | Deterministically extractable? | Why / what the AST gives |
| --- | --- | --- |
| **Domain / class** | **Yes (v1)** | types (`struct`/`enum`/`class`/`protocol`), their fields + field types, variants, and structural relationships (field-of → association, enum-variant → composition, trait/protocol impl → realization). Multiplicities from `Vec<T>`/`Option<T>`/`[T]?`. |
| **Component** | **Yes (v1)** | module/crate boundaries from the build manifest (`Cargo.toml` workspace members, `Package.swift` targets) + cross-module `use`/`import` edges = dependency arrows. |
| **State machine** | **Partial** | enum-with-data + transition fns are extractable as *candidates*; which transitions are user-meaningful stays a judgment call → agent confirms. |
| **Sequence** | **No (v1)** | needs call-graph + **ordering** + alternate/failure flows; a static caller/callee graph is not a runtime trace. Agent-assisted (can be *seeded* by a call-graph later). |
| **Use case** | **No** | actors and user goals are intent, absent from the AST. Agent-authored. |
| **`@desc` text** | **No** | one-line human meaning — agent overlay. |
| **`@sec` markers** | **Partial** | the brownfield signals in `mdd-map/SKILL.md` step 7 (guards, TLS, length checks) are pattern-detectable as *candidates*; the security judgement stays agent/PLAN-item-4. |

The split is the whole point: **structure is extracted, meaning is authored.**

## 5. Design

### 5.1 `mdd map-extract` — the verb

```
mdd map-extract [--lang rust|swift] [--only domain,components] \
                [--scope <files-from-map-scope>] [--json] [--write]
```

- Pure by default (prints a plan / the would-be PUML); `--write` emits to
  `.mdd/models/current/<kind>/<name>.puml` and the derived `source_links`.
- Deterministic and unit-tested, mirroring the plan-deterministic / execute split
  already used by `test-plan` and `map-scope`.
- Language auto-detected from build files when `--lang` omitted (reuse the
  detection pattern from `detect_test_profile`).

### 5.2 `LangAdapter` trait (language-pluggable)

```rust
trait LangAdapter {
    fn detect(root: &Path) -> bool;                 // build-file sniff
    fn types(&self, root: &Path) -> Vec<TypeNode>;  // name, kind, fields(name,type), variants
    fn modules(&self, root: &Path) -> Vec<Module>;  // boundary + import edges
}
```

- `RustAdapter` = today's `syn` engine, **enriched** to walk `ItemStruct`
  fields / `ItemEnum` variants / `ItemImpl` trait refs (it currently stops at
  name+span). This is the only net-new parsing work; the file walk + span infra
  exists.
- `SwiftAdapter` = `swift-syntax` (SwiftPM lib) or `sourcekitten`; same trait.
- The core emitter (`TypeNode[] → class PUML`, `Module[] → component PUML`) is
  **language-agnostic** — adapters only produce the neutral node model.

### 5.3 Sequences stay agent-assisted (explicit)

v1 does **not** emit sequences. A static call graph can later *seed* a sequence
skeleton (participants + candidate calls) for the agent to order and annotate,
but ordering, alternate flows, and failure paths require judgment a static graph
can't supply. `/mdd-map` keeps authoring sequences; `map-extract` leaves them
untouched.

### 5.4 Marker reconciliation + stable-id continuity (the hard part)

Generated PUML must **not** clobber the human/agent overlay (`@desc`, `@sec`,
hand-tuned `@ref`). Rule: **structure is owned by the extractor; semantic markers
are owned by the author; regeneration is a merge, not an overwrite.**

- **Stable ids:** an extracted node's `@id` is derived deterministically from its
  fully-qualified symbol (e.g. `DOM-<HASH(crate::module::Type)>` or a
  slugified FQN), so re-running yields the *same* id for the *same* type — no
  churn, and existing `@desc`/`@sec`/trace links keep resolving.
- **Merge on regen:** keep existing `@desc`/`@sec`/manual `@ref` lines for ids
  that still exist; add nodes for new types; mark nodes whose backing symbol
  vanished as removed (feeding the existing cycle-diff machinery). A small
  "overlay block" convention (markers the extractor never rewrites) keeps the
  boundary explicit.
- This is the riskiest area and the main reason for sign-off before build (§7).

### 5.5 `source_links` as a generation output

When the emitter writes a node from `(path, symbol)` it **emits the
`source_link`** at the same time. Hand-authoring trace entries for extracted ids
disappears; `/mdd-validate`'s existing source-link existence check and
`/mdd-review` pass-3 forward parity then validate them for free. Agent-authored
ids (use cases, sequences) still get hand-authored links.

### 5.6 Composition with `map-scope`

`map-scope` already answers "which current diagrams could the changed code have
touched." Feed its `affected_files` to `map-extract --scope` so extraction
re-derives only those — the two are complementary: `map-scope` *targets*,
`map-extract` *produces*. For extractable kinds, `map-extract` can **subsume**
the agent re-map step entirely inside the `/mdd-cycle` loop; the agent only
re-runs for the semantic kinds in scope.

## 6. Worked sketch (this repo)

`crates/mdd-core/src/lib.rs` defines `MapScopePlan { cycle, affected_files,
scope_escapes, full_remap }`. `map-extract --only domain` would emit
`current/domain/scoped-remap.puml` with a `MapScopePlan` class (4 typed fields),
`@id(DOM-MAP-SCOPE-PLAN)` derived from its FQN, and the `source_link` to
`lib.rs::MapScopePlan` — i.e. exactly the file hand-written in cycle 0030, but
for free. The `@desc` line (human meaning) would remain an agent overlay.

## 7. Risks & open questions (for sign-off)

1. **Merge/overwrite safety (highest risk).** Regeneration must never drop an
   author's `@desc`/`@sec`. Needs the overlay-ownership convention nailed down
   before any code. *Open: explicit `@gen`/`@manual` marker fences, or a
   3-way merge against the last generated snapshot?*
2. **Stable-id scheme.** FQN-hash vs slug; collisions; renames (a renamed type
   reads as remove+add unless we track moves). *Open: accept rename = churn in
   v1?*
3. **Diagram aesthetics.** Hand-drawn diagrams group/relate for readability;
   a naïve dump can be noisy. *Open: how much layout heuristics in v1?*
4. **Cross-language fidelity.** Swift generics/protocols/extensions don't map 1:1
   to the neutral node model. *Open: lowest-common-denominator node model, or
   per-language extensions?*
5. **Scope of v1.** Recommend domain **first** (highest value, cleanest AST
   mapping), components second, defer sequences/state.

## 8. Phased delivery (after sign-off, ≈2–3 cycles)

- **Cycle A — neutral model + Rust domain.** `LangAdapter` trait, `RustAdapter`
  enriched for fields/variants/edges, `TypeNode → class PUML` emitter,
  `mdd map-extract --only domain` (pure + `--write`), FQN-hash ids,
  `source_link` emission, merge-preserving `@desc`/`@sec`. Unit-tested against
  this repo's own `lib.rs`.
- **Cycle B — components + `map-scope` composition.** Module/import → component
  PUML; wire `--scope`; teach `/mdd-cycle` to run `map-extract` for extractable
  kinds and only invoke the agent `/mdd-map` for semantic kinds.
- **Cycle C — Swift adapter.** `SwiftAdapter` so the guest repo extracts
  natively; prove the trait boundary.

## 9. Decisions needed before any code

1. Confirm the **extractable/agent-only split** in §4 (esp. sequences out of v1).
2. Pick the **overlay-ownership / merge** model in §7.1.
3. Pick the **stable-id scheme** in §7.2.
4. Confirm **domain-first** phasing and that the MDD-tool repo (Rust) is the
   first proving ground, Swift guest repo second.
