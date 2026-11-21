import { expect, test } from '@playwright/test'
import { newProject, newUser } from './helpers'

test('viewer is read-only; removed member is disconnected', async ({ browser }) => {
  const owner = await newUser(browser, 'Olga')
  await newProject(owner, 'Roles test')
  await owner.locator('.add-bar__side--student').getByRole('button', { name: '+ Latin' }).click()
  await owner.keyboard.type('Salve')
  await owner.keyboard.press('Escape')

  await owner.getByRole('button', { name: 'Share', exact: true }).click()
  const share = owner.getByRole('dialog')
  await share.getByLabel('Role').last().selectOption('viewer')
  await share.getByRole('button', { name: 'Create link' }).click()
  const link = await share.getByRole('textbox', { name: 'Invite link', exact: true }).inputValue()
  await owner.keyboard.press('Escape')

  const viewer = await newUser(browser, 'Vic')
  await viewer.goto(link)
  await viewer.getByRole('button', { name: 'Join project' }).click()
  await expect(viewer.getByText('View only')).toBeVisible()
  const card = viewer.locator('.card').filter({ hasText: 'Salve' })
  await expect(card).toBeVisible()
  await expect(viewer.getByRole('button', { name: '+ Latin' })).toHaveCount(0)
  await card.dblclick()
  await viewer.keyboard.type('hack')
  await viewer.keyboard.press('Control+k')
  await expect(viewer.locator('textarea')).toHaveCount(0)
  await expect(viewer.locator('.card')).toHaveCount(1)
  await expect(card).not.toContainText('hack')

  // Owner removes the member.
  await owner.getByRole('button', { name: 'Share', exact: true }).click()
  await owner.getByRole('button', { name: 'Remove Vic' }).click()
  await owner.getByRole('button', { name: 'Remove', exact: true }).last().click()
  await expect(viewer.getByRole('alert').filter({ hasText: 'Disconnected from the project' })).toBeVisible({ timeout: 20_000 })
  await expect(viewer.getByText('You were removed from this project.').first()).toBeVisible()
  await expect(viewer.locator('.status')).toHaveText('Disconnected')
  // It stays closed rather than reconnecting.
  await viewer.waitForTimeout(4000)
  await expect(viewer.locator('.status')).toHaveText('Disconnected')
})
