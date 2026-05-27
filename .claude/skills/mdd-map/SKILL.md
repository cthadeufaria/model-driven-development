---
name: mdd-map
description: Derive the current view of the system from existing code into .mdd/models/current/
---

# MDD Map

You are an MDD, UML, PlantUML, and OCL specialist for the **current** view of the system.

Start by reading `.mdd/docs/mdd-workflow.md` and `.mdd/docs/uml-and-ocl-guide.md`. Treat `.mdd/models`, `.mdd/constraints`, and `.mdd/trace.yml` as authoritative MDD state.

Use this skill to derive the current view from existing code. Diagrams go into `.mdd/models/current/`.

## Preconditions

- Code must exist for the scope being mapped. If the repository (or the area the user mentioned) is empty, do not run; suggest `/mdd-generate` instead.
- Default scope: produce or refresh the **use-case diagram only**. Extend to other diagram types only when the user asks (e.g. "map domain and components") or when the existing use-case view is already current and they need more detail.

## Scoped re-map (driven by `mdd map-scope`)

When a caller hands you a **scope** — a set of current-side diagram files (as the `/mdd-cycle` loop does each iteration, from `mdd map-scope --cycle <N> --json`) — re-derive **only those files** and leave the rest of `.mdd/models/current/` **byte-identical**. This is the common case inside a cycle's implement→map loop, where only the source `/mdd-implement` just wrote can have changed; re-deriving the whole tree every pass is wasted work. Rules:

- **`full_remap: true`** in the plan (a changed source file resolved to **no** `source_link`, listed under `scope_escapes`) means the narrowed set cannot be trusted → ignore `affected_files` and re-map the **whole** current tree. Never silently narrow past an escape.
- **`full_remap: false`** → re-derive exactly the `affected_files`. Do not touch, reformat, or re-emit any other current file (preserving their `@id`s and `@ref`s by leaving them untouched).
- The plan's changeset comes from the same Rust/`src/` engine as `mdd map-status` (`git diff` of `.rs` files under a crate `src/`). A model-bearing change **outside** that surface (rare in this repo) won't appear in the plan — if you know you edited such a file, widen the re-map to cover it.
- `/mdd-validate` runs whole-tree afterward regardless, so a too-narrow scope is caught as a structural error rather than silently shipped — but prefer correctness up front via `full_remap`.

Outside a scope (a bare `/mdd-map`, or no plan handed in) behave exactly as before: default to the use-case view, extend on request.

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
9. **Discover native tests (brownfield coverage).** Find existing tests in the code (`#[cfg(test)]`, `tests/`, `__tests__`, `test_*.py`, Playwright e2e specs) and refresh the unified trace `tests` links to the model `@id`s they exercise, so a mapped repo starts with real coverage. Discovery is additive and structural — link what you find; never author or run tests here.
10. Hand off to `/mdd-validate`.
