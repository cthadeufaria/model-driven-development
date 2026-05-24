# mdd mockups

High-fidelity React renders of the MDD Salt mockups, paired with the contracts
under `.mdd/models/**/mockups/`. Each render is served at `/mockup/<slug>` and
mirrors a `<slug>.puml` Salt mockup: every `@ui-element(UIC-X)` in the contract
appears here as `data-testid="UIC-X"`.

## Commands

```bash
npm install                       # install deps (run once)
npm run dev                       # serve the mockups at http://localhost:4317
npm --prefix mockups run test:ui  # run the Playwright Salt/React parity specs
```

Run these from this `mockups/` directory, or with `--prefix mockups` from the
repo root.

## Parity specs

The parity specs live at `.mdd/tests/ui/<slug>.spec.ts` — the MDD-conventional
location, linked from `.mdd/trace.yml` under `generated_ui_tests`. Because that
folder is outside this app's `node_modules`, the `test:ui` script sets
`NODE_PATH` so the external specs can resolve `@playwright/test`. Each spec
parses the `UIC-` ids straight from the Salt contract, so it cannot drift from
the diagram: if the Salt adds an element, the spec fails until the render
exposes its `data-testid`.

## Adding a mockup

`/mdd-generate` authors both parts of an implementation-ready mockup: the Salt
contract (`.mdd/models/objective/mockups/<slug>.puml` with
`@ui-route(/mockup/<slug>)`) and the React render here
(`src/mockups/<slug>.tsx`), plus the parity spec. Register the new route in
`src/main.tsx`.
