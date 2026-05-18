# MDD Deploy Profile

`/mdd-deploy` is a **utility skill, not a workflow gate** — a sibling of
`/mdd-render`. It guides a deployment by producing a UML deployment diagram, a
copy-pasteable command runbook, and generated Infrastructure-as-Code. It never
executes a deploy command, and nothing it writes participates in the
`current ↔ objective` parity gate. `/mdd-validate`, `/mdd-review`, and the
`/mdd-cycle` parity loop never read `.mdd/deploy/`.

A UML deployment diagram has no code-derived counterpart (topology lives in
infrastructure, not application source), so it could never reach parity — by
design it is kept out of the gate.

## Outputs

- `.mdd/deploy/<target>/diagram.puml` — a true UML **deployment** diagram:
  nodes, the deployed artifact, and annotated communication paths/protocols.
- `.mdd/deploy/<target>/runbook.md` — ordered, numbered steps. Every step
  states the exact command, the directory / cloud context to run it in, and the
  required env/secret values. A **STOP / confirm** marker precedes every
  state-changing or go-live step.
- The generated IaC, written into the **target repo** (cross-repo output is
  fine because the skill is non-gated guidance).

`/mdd-render` is extended to rasterize `.mdd/deploy/**/*.puml` to its
deterministic `.mdd/rendered/deploy/**/*.svg` mirror, so the deployment diagram
is inspectable like every other diagram. Additive and non-gating.

## v1 target: `azure-container-apps`

v1 supports exactly one target — Azure Container Apps for the sibling repo
`../atlas-ate-server`. Any other target: report "not yet supported" and stop.
The multi-target profile mechanism (Homebrew, web, app-bundle) is deferred.

Deployment-diagram node vocabulary for this target:

- Azure Container Registry (the server container image artifact).
- Container Apps environment + the app revision (external ingress on `8080`,
  non-root container, `NODE_ENV=production`).
- Azure Database for PostgreSQL (TLS required, `sslmode=require`).
- Azure Key Vault (every secret; Container Apps secret references).
- Azure OpenAI.
- A separate Container Apps **job** for the database migration, ordered before
  any revision traffic routing.

## Documentation-only security stereotypes

For visual consistency the deployment diagram MAY reuse the existing stereotype
vocabulary — `<<Encrypt>>`, `<<ByPassing>>`, `<<Flooding>>`, `<<Expiration>>` —
as plain PlantUML stereotypes and notes. These are **documentation only**. They
are NOT the gated security-marker mechanism: `/mdd-deploy` is outside the
security-parity gate, so do not write `@sec(...)` markers here and never mistake
these stereotypes for parity-checked guards.

## Invariant checklist (atlas-ate-server, v1)

The diagram annotates and the runbook enforces, as explicit guard steps:

1. **Secrets only from Key Vault** — Container Apps secret references resolve to
   Key Vault; never in image layers, source, or `.env`. Provision via
   `az keyvault secret set` + Container Apps secret references.
2. **App Attest required in staging/production** — no `APP_ATTEST_BYPASS`; real
   `APP_ATTEST_TEAM_ID` / `APP_ATTEST_BUNDLE_ID`,
   `APP_ATTEST_ENVIRONMENT=production`.
3. **Billing production startup gate** — the runbook documents the full
   multi-factor guard (`ENABLE_PRODUCTION_BILLING=I_UNDERSTAND_BILLING_IS_LIVE`,
   `APP_ENV=production`, `DATABASE_MARKER=production`,
   `PUBLIC_HOST=api.atlas.codes`, `APPLE_ENVIRONMENT=Production`, Apple PKI
   config, App Attest prod identity, no local `APPLE_STOREKIT_JWKS`) behind an
   explicit "this goes live" confirm step.
4. **BYOK never touches the server** — the diagram shows the BYOK path bypassing
   the Container App entirely (no node/edge into the server).
5. **TLS + at-rest encryption to Azure DB for PostgreSQL** — `sslmode=require`;
   annotate `<<Encrypt>>` on the DB communication path.
6. **Migrations as an explicit pre-traffic job** — not auto-migrate on boot;
   ordered before revision traffic routing.
7. **Non-root container, port 8080 ingress, `NODE_ENV=production`.**
