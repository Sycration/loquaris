import { expect, test, type Page } from '@playwright/test'
import { newProject, newUser } from './helpers'

async function addLatin(page: Page, text: string) {
  await page.getByRole('button', { name: '+ Latin' }).first().click()
  await page.keyboard.type(text)
  await page.keyboard.press('Escape')
}

/** Entries without a recording last 10 s each, so a few of them are a minute. */
async function addFillers(page: Page, count: number) {
  for (let i = 0; i < count; i++) {
    await page.getByRole('button', { name: '+ English' }).click()
    await page.keyboard.type(`wait ${i + 1}`)
    await page.keyboard.press('Escape')
  }
}

test('spaced repetition flags: in a lesson, on a lesson footer, and the project switch', async ({ browser }) => {
  const page = await newUser(browser, 'Sera')
  await newProject(page, 'Spaced test')

  // Lesson 1: rosa at 0:00, due at 1:00; the entry starting at 1:00.6 is the
  // seventh, so it carries the flag.
  await addLatin(page, 'rosa')
  await addFillers(page, 6)
  await expect(page.locator('.card')).toHaveCount(7)
  const flag = page.locator('[data-due-flag]')
  await expect(flag).toHaveCount(1, { timeout: 30_000 })
  await expect(flag).toHaveText('⚑ rosa')
  await expect(page.locator('.card').nth(6).locator('[data-due-flag]')).toHaveCount(1)
  await expect(flag).toHaveAttribute('title', /^Due for review — last heard 1:0[01] earlier, entry 1$/)
  const section = page.getByRole('region', { name: 'Due for review in lesson 1' })
  await expect(section).toContainText('rosa')
  await expect(page.locator('[data-due-footer]')).toHaveCount(0)

  // Lesson 2: rosa again a day later, then 5 entries: the lesson ends at
  // 1:00.5, so nothing starts at the 1:00 deadline and the lesson is flagged.
  await page.getByRole('button', { name: 'New lesson' }).click()
  await page.getByRole('region', { name: 'Lesson 2', exact: true }).locator('.lesson-header__number').click()
  await expect(page.getByText(/^Adding to lesson 2/)).toBeVisible()
  await addLatin(page, 'rosa')
  await addFillers(page, 5)
  const lesson2 = page.getByRole('region', { name: 'Lesson 2', exact: true })
  await expect(lesson2.locator('.card')).toHaveCount(6)
  await expect(lesson2.locator('[data-due-footer]')).toHaveText('⚑ Not reviewed in this lesson: rosa', { timeout: 30_000 })
  await lesson2.locator('.lesson-header__number').click()
  await expect(page.getByRole('region', { name: 'Due for review in lesson 2' })).toContainText('last heard in lesson 2')
  // Lesson 1 still has its own flag.
  await expect(flag).toHaveCount(1)

  // Clicking a flag reveals where the word was last heard: lesson 1's first entry.
  await flag.click()
  await expect(page.locator('.card').first()).toHaveClass(/card--selected/)
  await expect(section).toBeVisible()
  await section.getByRole('button', { name: /rosa/ }).click()

  // The project switch: off hides every flag, on brings them back.
  await page.locator('.editor-header__title').click()
  const toggle = page.getByLabel('Spaced repetition flags')
  await expect(toggle).toBeChecked()
  await toggle.uncheck()
  await expect(flag).toHaveCount(0)
  await expect(page.locator('[data-due-footer]')).toHaveCount(0)
  await expect(page.locator('[data-due-section]')).toHaveCount(0)
  await toggle.check()
  await expect(flag).toHaveCount(1, { timeout: 30_000 })
  await expect(page.locator('[data-due-footer]')).toHaveCount(1)
  await page.getByRole('button', { name: 'Done' }).click()
})
