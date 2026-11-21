import { expect, test, type Page } from '@playwright/test'
import { newProject, newUser } from './helpers'

const card = (page: Page, text: string | RegExp) => page.locator('.card').filter({ hasText: text })

async function addEntry(page: Page, key: string, text: string) {
  await page.keyboard.press(`Control+${key}`)
  await page.keyboard.type(text)
  await page.keyboard.press('Escape')
}

const menuName = (item: string) => new RegExp(`^${item.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}(\\s|$)`)

async function menu(page: Page, entry: string | RegExp, item: string) {
  await card(page, entry).first().click({ button: 'right' })
  await page.getByRole('menuitem', { name: menuName(item) }).click()
}

test('entries: add, edit, context menu, multi-select, undo, lessons, details', async ({ browser }) => {
  const page = await newUser(browser, 'Edith')
  await newProject(page, 'Editor test')

  await expect(page.getByRole('region', { name: 'Lesson 1', exact: true })).toBeVisible()

  // Add bar + shortcuts.
  await page.getByRole('button', { name: '+ Latin' }).first().click()
  await page.keyboard.type('Salve')
  await page.keyboard.press('Escape')
  await addEntry(page, 'J', 'Hello')
  await addEntry(page, 'K', 'Ave')
  await addEntry(page, 'L', 'Valē')
  await expect(page.locator('.card')).toHaveCount(4)
  await expect(page.getByRole('group', { name: 'Teacher, Latin' })).toHaveCount(2)
  await expect(page.getByRole('group', { name: 'Student, Latin' })).toHaveCount(1)
  await expect(page.getByRole('group', { name: /^Teacher, (?!Latin)/  })).toHaveCount(1)

  // Edit text via the pencil.
  await card(page, 'Hello').hover()
  await card(page, 'Hello').getByRole('button', { name: 'Edit entry' }).click()
  await page.keyboard.press('End')
  await page.keyboard.type(' world')
  await page.keyboard.press('Escape')
  await expect(card(page, 'Hello world')).toBeVisible()

  // Context menu.
  await menu(page, 'Hello world', 'Duplicate')
  await expect(card(page, 'Hello world')).toHaveCount(2)
  await menu(page, 'Valē', '← Move to teacher')
  await expect(page.getByRole('group', { name: 'Student, Latin' })).toHaveCount(0)
  await menu(page, 'Ave', 'Move to student →')
  await expect(page.getByRole('group', { name: 'Student, Latin' })).toHaveCount(1)
  await menu(page, 'Hello world', 'Switch to Latin')
  await expect(page.getByRole('group', { name: /^Teacher, Latin/ })).toHaveCount(3)
  const order = async () => page.locator('.card .card__text').allTextContents()
  const before = await order()
  const last = before.length - 1
  await menu(page, before[last], 'Move up')
  const after = await order()
  expect(after[last - 1]).toBe(before[last])
  await menu(page, before[last], 'Move down')
  expect(await order()).toEqual(before)

  // Multi-select with shift-click, then delete selected.
  await page.locator('.card').first().click()
  await page.locator('.card').nth(1).click({ modifiers: ['Shift'] })
  await expect(page.locator('.card--selected')).toHaveCount(2)
  const count = await page.locator('.card').count()
  await page.locator('.card--selected').first().click({ button: 'right' })
  await page.getByRole('menuitem', { name: menuName('Delete selected') }).click()
  await expect(page.locator('.card')).toHaveCount(count - 2)

  // Undo / redo.
  await page.keyboard.press('Control+z')
  await expect(page.locator('.card')).toHaveCount(count)
  await page.keyboard.press('Control+Shift+z')
  await expect(page.locator('.card')).toHaveCount(count - 2)
  await page.keyboard.press('Control+z')

  // Lessons.
  await page.getByRole('button', { name: 'New lesson' }).click()
  await expect(page.getByRole('region', { name: 'Lesson 2', exact: true })).toBeVisible()
  await page.getByRole('textbox', { name: 'Lesson 2 name' }).pressSequentially('Second')
  await expect(page.getByRole('textbox', { name: 'Lesson 2 name' })).toHaveValue('Second')
  await page.getByRole('region', { name: 'Lesson 2', exact: true }).getByRole('button', { name: 'Collapse lesson', exact: true }).click()
  await expect(page.getByRole('region', { name: 'Lesson 2', exact: true }).getByRole('button', { name: 'Expand lesson' })).toBeVisible()

  // Project details gaps.
  await page.locator('.editor-header__title').click()
  const gap = page.getByLabel('Gap, same speaker (s)')
  await gap.fill('1.5')
  await page.getByLabel('Gap, speaker changes (s)').fill('99')
  await expect(page.getByText('Use a value from 0 to 60 seconds.')).toBeVisible()
  await page.getByRole('button', { name: 'Done' }).click()
  await page.locator('.editor-header__title').click()
  await expect(page.getByLabel('Gap, same speaker (s)')).toHaveValue('1.500')
  await page.keyboard.press('Escape')

  // Save.
  await page.keyboard.press('Control+s')
  await expect(page.getByText('All changes saved')).toBeVisible()
})

test('undo only reverts own edits', async ({ browser }) => {
  const alice = await newUser(browser, 'Ann')
  await newProject(alice, 'Undo test')
  await alice.getByRole('button', { name: 'Share', exact: true }).click()
  await alice.getByRole('button', { name: 'Create link' }).click()
  const link = await alice.getByRole('textbox', { name: 'Invite link', exact: true }).inputValue()
  await alice.keyboard.press('Escape')
  const bob = await newUser(browser, 'Ben')
  await bob.goto(link)
  await bob.getByRole('button', { name: 'Join project' }).click()
  await expect(bob.getByRole('main', { name: 'Script' })).toBeVisible()

  await addEntry(alice, 'K', 'Alpha')
  await expect(card(bob, 'Alpha')).toBeVisible()
  await addEntry(bob, 'K', 'Beta')
  await expect(card(alice, 'Beta')).toBeVisible()
  await alice.keyboard.press('Control+z')
  await expect(card(alice, 'Alpha')).toHaveCount(0)
  await expect(card(alice, 'Beta')).toBeVisible()
  await expect(card(bob, 'Beta')).toBeVisible()
})
