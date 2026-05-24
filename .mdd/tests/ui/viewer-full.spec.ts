import { test, expect } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

// UIT-MCK-VIEWER-FULL — Salt <-> React parity for MCK-VIEWER-FULL.
// The Salt contract is the single source of truth: every @ui-element(UIC-...)
// it declares must render as data-testid="UIC-..." at /mockup/viewer-full.
const saltPath = resolve(__dirname, '../../models/objective/mockups/viewer-full.puml')
const salt = readFileSync(saltPath, 'utf8')
const uicIds = [...salt.matchAll(/@ui-element\((UIC-[A-Z0-9-]+)/g)].map((m) => m[1])

test.describe('MCK-VIEWER-FULL — Salt/React parity', () => {
  test('every UIC- element in the Salt contract renders in the React mockup', async ({ page }) => {
    expect(uicIds.length, 'no UIC- elements parsed from the Salt contract').toBeGreaterThan(0)
    await page.goto('/mockup/viewer-full')
    for (const id of uicIds) {
      await expect(page.getByTestId(id), `missing data-testid="${id}"`).toBeVisible()
    }
  })
})
