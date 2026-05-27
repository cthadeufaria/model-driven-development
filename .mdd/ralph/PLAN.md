# Ralph plan

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

> **Theme: cut the AI token cost of the `current` side.** The expensive part of every
> cycle is `/mdd-map` re-deriving `current/` from code with an AI agent. `current` is a
> faithful mirror of code, so most of that work is mechanical and either redundant
> (re-deriving unchanged diagrams) or deterministically computable (AST → PlantUML).
> The items below go from the smallest proven win to the larger methodology shifts.
> The objective side (`/mdd-generate` from a description) and the parity gate
> (`validate`/`review`) stay AI/agent work — they encode intent, not a code mirror.
>
> **Note for whoever picks these up — correct two premises from the source review before acting:**
> `mdd-cli` is **not** "init and clean only" — it already ships deterministic verbs
> `validate`, `review`, `map-status`, `test-plan`, `test-detect`, `render`, `arch`,
> `context` (`crates/mdd-cli/src/main.rs`). And cycle snapshots are not pure git
> reinvention — the viewer reads `.mdd/cycles/` for superposed before/after diffs and
> the whole-map. The waste the review identifies is real; the mechanism descriptions are off.

- [x] **Scoped re-map: `mdd map-scope` verb + scope-aware `/mdd-map` in the cycle loop.** Today the cycle loop (`/mdd-validate → /mdd-implement → /mdd-map → /mdd-validate → /mdd-review`, repeated on every review mismatch) re-runs `/mdd-map` over the whole `current/` tree (82 puml files / ~8.5k lines + a ~1.5k-line `trace.yml` rewrite) on each pass, even though between two passes only the source `/mdd-implement` just wrote can have changed. Add a deterministic verb `mdd map-scope --cycle <N> --json` that returns the exact set of `current/` diagram files that may have changed, by reusing the existing `map-status`/`mdd_core::traceability` source-link engine: invert `trace.yml` `source_links` over the cycle manifest's `touched_files` (→ affected current `@id`s → their files) **unioned with** the manifest's `scope` objective `@id`s resolved to their current-side concepts. It must also list any `touched_files` entry that resolves to **no** `source_link` as a `scope_escape` that forces a widen-to-full map of that area — never silently narrow (surface the ambiguity). Then teach `/mdd-map` to accept that scope and re-derive only those files, leaving the rest of `current/` byte-identical (ids/refs in untouched files are preserved by construction; `mdd validate` still runs whole-tree afterward as the safety net). Wire `/mdd-cycle`'s loop step to call `map-scope` and pass the result to `/mdd-map` on each iteration — biggest win on re-loops; also helps whole-model (empty-`scope`) cycles via `touched_files`. Keep the plan-deterministic / execute-in-skill split (like `test-plan`/`deploy`). Savings scale with iteration count, i.e. exactly the cycles that burn most today.
- [x] **Design proposal first: deterministic current-side extraction (AST → PlantUML).** → `docs/proposals/deterministic-current-extraction.md` (awaiting sign-off; no code lands until §9 decisions are made). Investigate replacing the *mechanical* parts of `/mdd-map` with a zero-AI extractor so the agent stops hand-writing diagrams that mirror code. Scope the proposal to: (a) domain/class + component diagrams are deterministically derivable from a language AST + module/manifest boundaries (the guest repo is Swift → `swift-syntax`/SourceKit; design the verb language-pluggable, the MDD tool itself is Rust); (b) sequence diagrams need call-graph/ordering analysis and likely stay agent-assisted — say so explicitly; (c) if `current/` is generated, `source_links` become a generation output, so trace authoring shrinks to "implicit from extraction" for extracted ids. Output: a proposal under `docs/proposals/` with the verb surface, the AST-extractable vs agent-only diagram split, how generated output reconciles with the existing `@id`/`@desc`/`@sec` markers and stable-id continuity across runs, and how it composes with (or subsumes) `map-scope` above. Do **not** implement before the proposal is signed off — this is a methodology shift, not a described feature; route to general tools, not `/mdd-cycle`.
- [x] **Design proposal first: cycle history vs. git — stop copying the whole tree.** → `docs/proposals/cycle-history-via-git.md` (awaiting sign-off; Phase 0 `whole/` removal can ship first). The cycle Close copies the entire `current/` tree into `before/` and `after/` (≈225 files/cycle observed) plus a `whole/` snapshot. Evaluate keeping only the artifacts the viewer actually needs (the semantic `.diff.puml` files + the accumulated whole-map) and deriving before/after from git refs recorded in the manifest instead of full-tree copies. Quantify the storage/IO saved, confirm the viewer can resolve a git ref (or define the minimal retained set if it can't), and confirm offline/replay recoverability is preserved. Proposal under `docs/proposals/`, signed off before any change.
- [x] **Design proposal first: make constraints and `@sec` markers executable, not inert.** → `docs/proposals/executable-constraints-and-sec.md` — gap analysis: mostly already designed (`tdd-and-test-assertion.md`) + shipped (security layer/tests, coverage OCL, red→green gate); inert here only because `test.layers` is empty. Genuine remaining delta = the unshipped `UT-`/OCL-unit slice. The source review argues OCL constraints and `@sec` PlantUML comments document rules that nothing enforces at runtime. Evaluate backing each `OCL-…` / `@sec(…)` with a real assertion or test (in the guest repo's native test layer) so the rule is enforced, not just drawn — tying into the existing diagram-driven-tests work (`project_diagram_driven_tests`, `test.layers`). Decide what stays as model-level documentation vs. what becomes a generated/linked executable check, and how `/mdd-review`'s security-parity gate relates to a passing assertion. Proposal under `docs/proposals/`, signed off before any change.

## Blocked

<!-- Ralph moves items here, with a one-line reason, when the parity gate can't be made to pass. -->
