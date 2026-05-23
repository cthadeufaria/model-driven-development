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
- The generated IaC in the **operator-confirmed dialect** (Bicep or
  Terraform — exactly one per run), written into the **target repo**
  (cross-repo output is fine because the skill is non-gated guidance).

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

## IaC dialect — operator-confirmed, one per run

The generated IaC is produced in exactly one dialect per run:

- **Bicep** — `infra/main.bicep` (+ Bicep modules); the Azure-native ARM
  DSL, applied with `az deployment`.
- **Terraform** — `infra/main.tf` (+ Terraform modules), the `azurerm`
  provider, and a **remote state backend** (`backend "azurerm"`); applied
  with `terraform plan` / `terraform apply`.

Exactly one dialect per run (Bicep XOR Terraform); the multi-emit and
non-Azure-provider cases are deferred.

**Mandatory confirmation — before any artifact.** The dialect must be
explicitly confirmed before the skill writes ANY deploy artifact (the
deployment diagram, the generated IaC, or the runbook). There is **no
silent default**: an unspecified or unconfirmed dialect halts the skill
with a blocking question — the same discipline as the landmine pause,
never demoted to a runbook STOP note. Nothing under `.mdd/deploy/` or the
target `infra/` is created until the operator confirms.

**Full parity between dialects.** Whichever dialect is confirmed, the
generated IaC declares the identical resource set and satisfies the
identical invariant checklist and secure-by-default network posture
below. The only differences are language, file structure, and (Terraform)
the remote state backend.

## Deployment purpose — operator-confirmed

Infrastructure sizing and posture depend on a fact the skill must not
guess: is this **dev, staging, or prod**? The skill resolves the
deployment purpose and **blocks for explicit operator confirmation**
before generating any purpose-driven default — the same discipline as the
dialect gate, with **no silent default** (never assume prod-grade or
dev-grade). The confirmed purpose then drives:

- **Azure Database for PostgreSQL tier / redundancy** — e.g. a burstable
  (B-series) tier for dev vs. a general-purpose / zone-redundant tier for
  prod. The skill must not emit a production-grade SKU for a dev
  deployment, nor an under-provisioned one for prod.
- **Backup retention** and other durability knobs.
- **Which network / doorway posture is recommended** (see
  Access-completeness).

The skill **surfaces** the purpose-appropriate options for the operator to
confirm; it does not bake one in. Purpose *recommends*; the operator
decides. Secure-by-default is preserved regardless of purpose — the vault
stays most-restrictive and any relaxation remains an explicit, surfaced
decision. Invariants: `OCL-DEPLOY-IAC-PURPOSE` (value in the set,
confirmed before any purpose-driven default) and
`OCL-DEPLOY-IAC-SURFACE-NOT-DECIDE` (surface the choice, never bake in an
answer).

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

## Secure-by-default

Automatable hardening is part of the guidance, not a deferred human
decision. The generated IaC must never emit a secret or data store more
openly network-reachable than its peers. Rule: choose the most
restrictive network posture consistent with the connectivity the runbook
actually requires — **runtime AND provisioning** — and relax it only via
an explicit, surfaced decision. Hardening that creates a new requirement
(a locked vault now needs a writer path) must propagate that requirement:
surface the doorway, never bake one in and never silently relax.

This posture is identical for both IaC dialects (full parity) — the skill
hardens whichever dialect was confirmed, never one more than the other.

Concretely for the v1 target: when Postgres and Azure OpenAI are private,
Key Vault must default to the most restrictive posture — in Bicep
`networkAcls.defaultAction: 'Deny'` with `bypass: 'AzureServices'`, in
Terraform the equivalent `network_acls { default_action = Deny, bypass =
AzureServices }` (or a private endpoint) — never public network access
with an `Allow` default. A Key Vault left publicly reachable while its
peers are private is a secure-by-default failure the skill must fix
automatically, not a decision it may defer.

## Access-completeness — every secured store has both ends

Before the deployment diagram is "done", account for **both ends** of
every secured store — Azure Key Vault, Azure Database for PostgreSQL, the
container registry, the remote state backend. For each, name:

- **who READS it** (usually the running app, via its managed identity), and
- **who WRITES / provisions it** (usually the **deployer** — the operator
  or CI that runs `apply`),

and give each a concrete **identity** and a **network path the chosen
posture actually admits**. The recurring failure this prevents: a vault
modelled only with the app's read path, hardened to the most-restrictive
posture, with **no writer and no admitted path** — so `apply` fails
writing the secrets nobody was shown provisioning, on the path the diagram
never drew.

A store **read but never written**, or a **writer with no admitted path**
through a door the skill just hardened, is an **incomplete diagram** — the
skill **surfaces it as a blocking question** with the connectivity options,
framed by the deployment purpose:

- **allowlist the deployer's IP** — smallest change, keeps default-Deny;
  suited to a dev deployment from a workstation;
- **provision from inside the VNet** (a one-shot job, a jump host, a
  VNet-integrated Cloud Shell, or a self-hosted CI runner) — no public
  exposure; suited to prod;
- **a private tunnel** (VPN / Bastion) — reusable, highest setup.

The skill does **not** pick the option; it surfaces the menu and the
operator chooses. The IaC then emits **both** access grants — the reader's
read-only role and the writer's write role — plus the chosen connectivity.
Invariant: `OCL-DEPLOY-IAC-ACCESS-COMPLETE`.

## Landmine detection — mandatory pause

A **go-live landmine** is any choice whose correctness depends on a fact
**not grounded in the inputs the skill read** (`.mdd/models`, the target
repo `src/`, `Dockerfile`, `.env.example`, the operator's answers) — most
often a confident default the skill filled in for an unasked question. It
is **not a fixed list of known traps; recognize the shape.** When the skill
detects one it MUST pause with a **blocking clarification** (the same
discipline as `/mdd-cycle`) before writing the contradicting artifact, and
must NOT bury it as a runbook STOP note: a STOP note is procedural and easy
to skip past; a landmine is a surfaced decision the operator has to make
before go-live is even safe to attempt.

Three axes recur (the list grows as new shapes appear):

1. **App-config axis** — a generated config default contradicts what the
   shipped code supports. Worked example: defaulting
   `azureOpenAiUseManagedIdentity = true` while the shipped server
   authenticates Azure OpenAI only with an API key (no `@azure/identity`
   code path) would ship an app that cannot reach Azure OpenAI at go-live.
2. **Purpose axis** — sizing or posture chosen without knowing dev vs.
   prod. Worked example: emitting a production-grade PostgreSQL SKU for a
   dev / first deployment (≈30× the cost), or an under-provisioned one for
   prod. Resolved by the Deployment-purpose gate.
3. **Access-path axis** — a secured store hardened without a way in for
   whoever must reach it. Worked example: a Key Vault locked to
   default-Deny with only the app's read path modelled, so `apply` fails
   writing secrets the deployer has neither the role nor a network path to
   write. Resolved by the Access-completeness pass.

The general rule subsumes all three: surface the ungrounded choice; never
ship a confident default in its place.

## Known tradeoff (not auto-changed)

"Migrations before traffic" (invariant 6) is enforced **procedurally** —
an ordered pre-traffic migration job plus a runbook STOP — not as an
infrastructure interlock the platform could enforce structurally. This
weaker pattern is a documented, accepted tradeoff for v1. The skill
surfaces it here rather than silently choosing it, but does not
auto-change it; tightening it into a true interlock is deferred.
