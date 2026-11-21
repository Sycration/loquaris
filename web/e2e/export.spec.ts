import { expect, test, type Page } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { addStudentLatin, fakeMic, newProject, newUser, recordInto } from './helpers'

test.use(fakeMic)

async function openMenuItem(page: Page, name: string) {
  await page.getByRole('button', { name: 'Project menu' }).click()
  await page.getByRole('menuitem', { name: new RegExp(`^${name}`) }).click()
}

async function exportAs(page: Page, format: string) {
  await openMenuItem(page, 'Export')
  const dialog = page.getByRole('dialog', { name: 'Export' })
  await dialog.getByLabel('Format').selectOption(format)
  return dialog
}

test('export PDF, WAV, .loqr; import .loqr; version history', async ({ browser }) => {
  const page = await newUser(browser, 'Eva')
  await newProject(page, 'Export test')
  const card = await addStudentLatin(page, 'Salve mundē')

  // PDF.
  let dialog = await exportAs(page, 'pdf')
  let [download] = await Promise.all([page.waitForEvent('download'), dialog.getByRole('button', { name: 'Export', exact: true }).click()])
  expect(download.suggestedFilename()).toMatch(/\.pdf$/)
  expect(readFileSync((await download.path())!).subarray(0, 4).toString()).toBe('%PDF')
  await dialog.getByRole('button', { name: 'Close', exact: true }).last().click()

  // WAV blocked while incomplete, then allowed.
  dialog = await exportAs(page, 'wav')
  await expect(dialog.getByRole('alert')).toBeVisible()
  await expect(dialog.getByRole('button', { name: 'Export', exact: true })).toBeDisabled()
  await page.keyboard.press('Escape')
  const rec = await recordInto(page, card)
  await rec.getByRole('button', { name: 'Close', exact: true }).last().click()
  dialog = await exportAs(page, 'wav')
  await expect(dialog.getByRole('alert')).toHaveCount(0)
  ;[download] = await Promise.all([page.waitForEvent('download'), dialog.getByRole('button', { name: 'Export', exact: true }).click()])
  expect(download.suggestedFilename()).toMatch(/\.wav$/)
  expect(readFileSync((await download.path())!).subarray(0, 4).toString()).toBe('RIFF')
  await page.keyboard.press('Escape')

  // .loqr.
  dialog = await exportAs(page, 'loqr')
  ;[download] = await Promise.all([page.waitForEvent('download'), dialog.getByRole('button', { name: 'Export', exact: true }).click()])
  expect(download.suggestedFilename()).toMatch(/\.loqr$/)
  const loqrPath = test.info().outputPath('export.loqr')
  await download.saveAs(loqrPath)
  await page.keyboard.press('Escape')

  // Import it on the projects page.
  await page.goto('/')
  await page.locator('input[type=file][accept=".loqr"]').setInputFiles(loqrPath)
  await expect(page).toHaveURL(/\/p\//)
  await expect(page.locator('.card').filter({ hasText: 'Salve mundē' })).toBeVisible()
  await expect(page.locator('.card[aria-label*="has audio"]')).toHaveCount(1)

  // Version history: name, edit, restore.
  await openMenuItem(page, 'Version history')
  const history = page.getByRole('dialog', { name: 'Version history' })
  await history.getByLabel('Name current version').fill('Before change')
  await history.getByRole('button', { name: /^(Name|Save|Create)/ }).first().click()
  await expect(history.getByText('Before change')).toBeVisible()
  await page.keyboard.press('Escape')
  await addStudentLatin(page, 'Extra line')
  await expect(page.locator('.card')).toHaveCount(2)
  await openMenuItem(page, 'Version history')
  await history.locator('.history-item', { hasText: 'Before change' }).getByRole('button', { name: 'Restore' }).click()
  await page.getByRole('button', { name: 'Restore', exact: true }).last().click()
  await expect(page.getByText('Version restored.')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.locator('.card')).toHaveCount(1)
})
