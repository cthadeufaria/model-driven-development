# Design Proposal — Cycle History via Git (stop copying the whole tree)

**Status:** draft for review · **Date:** 2026-05-27 · **Author:** Ralph (PLAN item 3)
**Decision owner:** repo maintainer · **Delivery:** phased MDD cycles, *after* sign-off

> Nothing here is implemented. Deliverable = the proposal; no code lands before sign-off.

## 1. Summary

Every `/mdd-cycle` Close copies the **entire** `current/` tree into
`.mdd/cycles/<N>/before/` and `.mdd/cycles/<N>/after/`, **plus** a full copy of
the whole-map into `.mdd/cycles/<N>/whole/`. The result: `.mdd/cycles` is now
**18 MB across 3,961 files** (~250 files / 1.1 MB **per cycle**, growing every
cycle) — re-implementing, by hand, the one thing git already does perfectly:
versioned snapshots of tracked files. The models are committed to git on every
cycle anyway, so the `before/`/`after/` copies are **redundant with history**.

This proposal:

1. **Deletes the `whole/` snapshot outright** — grounded check shows **no code
   reads it** (it appears only in doc templates). Pure dead weight; safe to drop now.
2. **Replaces `before/`/`after/` full-tree copies with two git refs** recorded in
   the manifest (`before_rev`, `after_rev`). `cycle_diffs` reads file content at
   those refs via the `git show <ref>:<path>` helper **mdd-core already has**.
3. **Keeps** the artifacts the viewer actually consumes: the semantic
   `<diagram>.diff.puml` files and their rendered SVGs.

Net effect: `.mdd/cycles/<N>/` shrinks from ~250 files to *a manifest + the
handful of changed-diagram `.diff.puml`s* — the diff artifacts that are the whole
point — while full before/after recoverability is preserved through git.

## 2. Current state (grounded)

| Concern | Today | Where |
| --- | --- | --- |
| Storage | `.mdd/cycles` = **18 MB / 3,961 files**; per cycle ~250 files / 1.1 MB | measured |
| `before/` | full copy of `current/` at Open | `templates.rs` Close step; `cycle.rs` `before_dir` |
| `after/` | full copy of `current/` at Close | `templates.rs:427`; `cycle.rs` `after_dir` |
| `whole/` | full copy of `.mdd/map/` at Close | `templates.rs:443` |
| **Who reads `before/`+`after/`** | **only** `cycle_diffs(number)`, which `read_to_string`s each side to compute per-diagram element diffs | `cycle.rs:325-346`; sole caller `mdd-viewer/src/lib.rs:1044` |
| **Who reads `whole/`** | **nobody in code** — grep finds it only in `templates.rs` doc text | grounded: no reader in any crate |
| Viewer Diff mode | consumes `cycle_diffs(...)` (in-memory) + the rendered `.diff.svg` | `mdd-viewer/src/lib.rs:1044` |
| Git-at-ref read | mdd-core **already** reads a file at a ref via `git show <ref>:<path>`, absent = empty | `lib.rs:1609-1618` (used by `traceability_base`) |
| Models in git | committed every cycle (the cycle commit) | repo history |

**Reading:** `before/`/`after/` exist *only* to feed `cycle_diffs`; `whole/`
feeds nothing. The exact "read a tracked file as it was at commit X" capability
needed to replace them is **already in the codebase**.

## 3. Goals / non-goals

**Goals**

- Stop the per-cycle full-tree duplication; keep `.mdd/cycles/<N>/` to manifest +
  semantic diff artifacts.
- Preserve **full before/after recoverability** and the viewer's Diff mode,
  byte-for-byte.
- Reuse the existing `git show` helper and `cycle_diffs` shape — minimal surface.

**Non-goals**

- Changing the `.diff.puml` format or the rendered-SVG pipeline (those stay).
- Changing the whole-map *itself* (`.mdd/map/`) — only its redundant per-cycle
  `whole/` **copy** is dropped; `.mdd/map/` remains the live accumulator.
- Rewriting existing cycles' on-disk snapshots (back-compat; see §6).

## 4. Design

### 4.1 Drop `whole/` (phase 0, trivial)

Remove the "copy `.mdd/map/` → `.mdd/cycles/<N>/whole/`" step from the Close
flow (skill `templates.rs`). Nothing reads it. The "system picture as of cycle N"
it claimed to preserve is already recoverable from git at the cycle commit (and
the *live* `.mdd/map/` is the current picture). −~80 files/cycle immediately.

### 4.2 Manifest records refs, not copies

Extend `CycleManifest` with:

```yaml
before_rev: <sha>     # commit whose .mdd/models/current is the pre-cycle state
after_rev:  <sha>     # commit whose .mdd/models/current is the post-cycle state
```

- `before_rev` = `HEAD` captured at **Open** (the last cycle's commit / pre-work state).
- `after_rev` = the **cycle commit** itself. Chicken-and-egg: the commit sha isn't
  known until after commit. Resolve by either (a) a deferred manifest amend (write
  `after_rev` in a tiny follow-up commit / `--amend`), or (b) define `after_rev`
  implicitly as "the commit that closed cycle N" and resolve it at read time from
  git log (the cycle commit message already encodes `MDD cycle <N>`). **Open
  decision — §7.1.**

### 4.3 `cycle_diffs` reads from refs

`cycle_diffs(cycle)` keeps its signature and output (`Vec<CycleDiff>`); only its
*source* changes: instead of `collect_puml(before_dir)` / `collect_puml(after_dir)`,
enumerate the `current/` model paths and read each side via the existing
`git show <before_rev>:<path>` / `<after_rev>:<path>` helper (absent = empty,
which the helper already handles and which is exactly how adds/removes are
detected today). The viewer is **unchanged** — it still calls `cycle_diffs(n)`.

### 4.4 What stays on disk

`.mdd/cycles/<N>/` = `manifest.yml` + the changed-diagram `<rel>.diff.puml` files
(+ their rendered `.mdd/rendered/cycles/<N>/*.svg`). These are derived, small, and
the actual viewer payload — keeping them avoids needing git **or** a PlantUML
engine at view time for the common path.

## 5. Worked numbers

Cycle 0030 staged **319 files** (mostly before/after/whole). Under this proposal
it would have been: manifest + **4** `.diff.puml` + 4 rendered SVGs ≈ **9 files**.
Across the existing 30 cycles, `.mdd/cycles` drops from ~3,961 files / 18 MB to a
few hundred small files.

## 6. Backward compatibility & migration

- `cycle_diffs` falls back to the **old path** when `before_dir`/`after_dir`
  exist and the manifest has no `before_rev`/`after_rev` — so all 30 existing
  cycles keep working untouched.
- New cycles write refs and skip the copies. No history rewrite, no migration
  script required; optionally a one-shot `mdd cycles gc` could prune old
  before/after/whole dirs for repos that want the space back.

## 7. Risks & open questions (for sign-off)

1. **`after_rev` resolution (main design choice).** Amend-after-commit vs.
   resolve-from-log vs. a two-step "close then stamp" commit. Amend is cleanest
   but rewrites the just-made commit; resolve-from-log avoids that but couples to
   the commit-message convention. *Recommend: amend, since the cycle commit is
   local and unpushed at Close.*
2. **Git dependency at view time.** The viewer would need git for Diff mode *if*
   it ever needs to recompute diffs — but it consumes the pre-rendered `.diff.svg`
   for painting, so the common path stays git-free. Recomputation (rare) shells
   `git show`, already a dependency for review/freshness. *Acceptable.*
3. **Uncommitted / dirty working tree at Open.** `before_rev = HEAD` assumes the
   pre-cycle models are committed. They are (each cycle commits). Greenfield's
   first cycle: `before_rev` = empty-tree sentinel (the `git show` helper already
   treats absent as empty). *Covered.*
4. **GC / shallow clones.** A `gc`-pruned or shallow history could lose a
   referenced sha. Cycle commits are reachable from the branch, so normal `gc`
   won't drop them; shallow clones are an edge case. *Open: note in docs; not a
   v1 blocker.*
5. **Diff artifacts vs. git purity.** One could argue even `.diff.puml` is
   derivable from git on demand. But they're small, semantic, and the viewer's
   direct payload — keeping them is the pragmatic line. *Recommend keep.*

## 8. Phased delivery (after sign-off)

- **Phase 0 — drop `whole/`.** One skill-flow edit; nothing reads it. Immediate
  ~80 files/cycle saved. (Could ship as a tiny standalone cycle.)
- **Phase 1 — refs + ref-based `cycle_diffs`.** Manifest fields, the `after_rev`
  resolution decision (§7.1), `cycle_diffs` reads via `git show` with the
  old-path fallback, skill Close stops copying `before/`/`after/`. Unit-tested
  against a temp git repo (the harness added in cycle 0030 already does this).
- **Phase 2 (optional) — `mdd cycles gc`.** Prune legacy before/after/whole for
  space reclamation on demand.

## 9. Decisions needed before any code

1. `after_rev` resolution strategy (§7.1) — recommend amend.
2. Confirm `.diff.puml` + rendered SVG **stay on disk** (§7.5) — recommend yes.
3. Confirm Phase 0 (`whole/` removal) can ship independently and first.
