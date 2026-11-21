import { expect, type Browser, type Page } from '@playwright/test'

let counter = 0

/** A signed-in page for a newly registered user, in its own browser context. */
export async function newUser(browser: Browser, name: string): Promise<Page> {
  const context = await browser.newContext()
  const page = await context.newPage()
  const email = `${name.toLowerCase()}-${Date.now()}-${counter++}@example.com`
  await page.goto('/register')
  await page.getByLabel('Name').fill(name)
  await page.getByLabel('Email').fill(email)
  await page.getByLabel('Password', { exact: true }).fill('correct horse battery')
  await page.getByLabel('Confirm password').fill('correct horse battery')
  await page.getByRole('button', { name: 'Create account' }).click()
  await expect(page.getByRole('button', { name: 'New project' }).first()).toBeVisible()
  return page
}

/** Creates a project and opens it in the editor. */
export async function newProject(page: Page, name: string): Promise<void> {
  await page.getByRole('button', { name: 'New project' }).first().click()
  await page.getByLabel('Project name').fill(name)
  await page.getByRole('button', { name: 'Create project' }).click()
  await expect(page).toHaveURL(/\/p\//)
  await expect(page.getByRole('main', { name: 'Script' })).toBeVisible()
  // A new project opens its details, as on the desktop.
  await expect(page.getByRole('dialog', { name: 'Project details' })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('dialog')).toHaveCount(0)
}

/** Chromium flags for a fake microphone; use with `test.use`. */
export const fakeMic = {
  launchOptions: { args: ['--use-fake-device-for-media-stream', '--use-fake-ui-for-media-stream'] },
  permissions: ['microphone'] as string[],
}

/** Adds a student Latin entry via the add bar and returns its card. */
export async function addStudentLatin(page: Page, text: string) {
  await page.locator('.add-bar__side--student').getByRole('button', { name: '+ Latin' }).click()
  await page.keyboard.type(text)
  await page.keyboard.press('Escape')
  return page.locator('.card').filter({ hasText: text })
}

/** Records ~1.5 s into the entry's card through the recorder dialog, then closes it. */
export async function recordInto(page: Page, card: ReturnType<Page['locator']>) {
  await card.hover()
  await card.getByRole('button', { name: 'Record or import audio' }).click()
  const dialog = page.getByRole('dialog', { name: 'Record audio' })
  await dialog.getByRole('button', { name: /^Record/ }).click()
  await dialog.getByRole('button', { name: /^Stop/ }).waitFor()
  await page.waitForTimeout(1500)
  await dialog.getByRole('button', { name: /^Stop/ }).click()
  await expect(card).toHaveAttribute('aria-label', /has audio/, { timeout: 20_000 })
  return dialog
}

/** A one-second 16-bit mono WAV containing a sine tone. */
export function sineWav(seconds = 1, rate = 22050): Buffer {
  const n = Math.round(seconds * rate)
  const buf = Buffer.alloc(44 + n * 2)
  buf.write('RIFF', 0)
  buf.writeUInt32LE(36 + n * 2, 4)
  buf.write('WAVEfmt ', 8)
  buf.writeUInt32LE(16, 16)
  buf.writeUInt16LE(1, 20)
  buf.writeUInt16LE(1, 22)
  buf.writeUInt32LE(rate, 24)
  buf.writeUInt32LE(rate * 2, 28)
  buf.writeUInt16LE(2, 32)
  buf.writeUInt16LE(16, 34)
  buf.write('data', 36)
  buf.writeUInt32LE(n * 2, 40)
  for (let i = 0; i < n; i++) buf.writeInt16LE(Math.round(Math.sin((2 * Math.PI * 440 * i) / rate) * 12000), 44 + i * 2)
  return buf
}
