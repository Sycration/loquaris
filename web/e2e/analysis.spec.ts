import { expect, test, type Page } from '@playwright/test'
import { newProject, newUser } from './helpers'

const card = (page: Page, text: string | RegExp) => page.locator('.card').filter({ hasText: text })

async function addLatin(page: Page, text: string) {
  await page.getByRole('button', { name: '+ Latin' }).first().click()
  await page.keyboard.type(text)
  await page.keyboard.press('Escape')
}

test('search, new words, first use labels, auto-macronize', async ({ browser }) => {
  const page = await newUser(browser, 'Anna')
  await newProject(page, 'Analysis test')
  await addLatin(page, 'puella rosam amat')
  await addLatin(page, 'puellae in hortō')

  const search = page.getByRole('searchbox', { name: 'Search the project' }).or(page.getByLabel('Search the project'))
  await search.fill('puella')
  const panel = page.getByRole('region', { name: 'Search', exact: true })
  await expect(panel.locator('.analysis__summary')).toContainText(/\d/, { timeout: 30_000 })
  await expect(panel.getByRole('button', { name: 'puella', exact: true }).or(panel.locator('.analysis__lemma-name'))).toBeVisible()
  await expect(page.locator('mark.search-hit').first()).toBeVisible()
  await expect(page.getByRole('region', { name: /^Contexts in lesson/ }).locator('.analysis__context')).toHaveCount(2)
  await panel.getByRole('button', { name: 'Clear search' }).click()

  await expect(page.getByRole('region', { name: /^New in lesson 1/ }).locator('.chip').first()).toBeVisible()
  await expect(page.locator('.first-use').first()).toBeVisible()

  // Auto-macronize.
  await addLatin(page, 'Roma est in Italia')
  const before = await card(page, 'Roma est').locator('.card__text').innerText()
  await card(page, 'Roma est').click({ button: 'right' })
  await page.getByRole('menuitem', { name: /^Auto-macronize/ }).click()
  const dialog = page.getByRole('dialog', { name: 'Review Latin parsings' })
  const changed = async () => (await page.locator('.card').last().locator('.card__text').innerText()) !== before
  await expect.poll(async () => (await dialog.isVisible()) || (await changed()), { timeout: 20_000 }).toBeTruthy()
  if (await dialog.isVisible()) {
    await dialog.getByRole('button', { name: 'Apply' }).click()
    await expect(dialog).toHaveCount(0)
  }
  await expect.poll(changed).toBeTruthy()
  await expect(card(page, /ā|ē|ī|ō|ū/).first()).toBeVisible()
  await page.keyboard.press('Control+z')
  await expect(card(page, 'Roma est in Italia')).toBeVisible()
})
