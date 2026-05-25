# Architecture Source of Truth & Change Tracking

The **architectural source of truth (SoT)** for this project is the structured,
machine-readable spec under `.mdd/architecture/`. It is the authoritative record
of *what* the architecture is and *why* — beside the diagrams, which are the
visual model.

## What lives where

- `.mdd/architecture/components.yml` — the logical / deployable components: kind,
  the domain `@id`s each owns, dependencies, and tech.
- `.mdd/architecture/decisions.yml` — architecture decisions as data
  (ADR-as-data): id, title, status, context, decision, consequences, and the
  supersession chain. **Append-only** — the list is the decision *history*.
- `.mdd/architecture/constraints.yml` — cross-cutting rules the architecture
  must uphold, with rationale and `applies_to`.

These are **SeededOnce**: `mdd init` writes documented-but-empty templates;
`/mdd-kickoff` authors the initial spec from the agreed brief; thereafter the
content is owned by the project (`mdd init --force` never clobbers it).

## How it relates to the rest of `.mdd/`

- **Diagrams** (`.mdd/models/`) are the *visual* model and carry their own
  history via the whole-map (`.mdd/map/`, grown per cycle) and cycle diffs.
- **The brief** (`.mdd/docs/brief.md`) is the narrative kickoff alignment; the
  architecture SoT is its durable, structured continuation.
- `components.yml` references diagram `@id`s (`CMP-`/`DOM-`) so the structured
  spec and the diagrams stay in sync.

## How any agent tracks an architectural change

When you change the architecture (add/restructure a component, pick a new
technology, add a cross-cutting rule):

1. **Update the spec.** Edit the relevant `.mdd/architecture/*.yml`.
2. **Record the decision — append, don't rewrite.** Add a new entry to
   `decisions.yml`. If it changes a prior decision, set the old one's
   `status: superseded` and `superseded_by: <new id>`, and the new one's
   `supersedes: <old id>`. Never delete or edit an accepted decision — the file
   is the history.
3. **Keep `components.yml` in sync** with the component diagrams you touched.
4. **Commit.** The structured diff is captured in git (the interim history
   mechanism). A dedicated `mdd arch diff` / `mdd arch status` verb — folding
   SoT changes into the MDD snapshot/diff machinery and runnable detached from a
   cycle — is the planned follow-up.

## Invariants (see `.mdd/constraints/architecture.ocl`)

- Every decision has a `status` (`OCL-ARCH-DECISION-HAS-STATUS`).
- A `superseded` decision names its successor (`OCL-ARCH-SUPERSEDE-LINKED`).
- A component's `owns` / `depends_on` `@id`s resolve to real model ids
  (`OCL-ARCH-COMPONENTS-IN-SYNC`).
