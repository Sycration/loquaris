import { expect, test } from '@playwright/test'
import { addStudentLatin, fakeMic, newProject, newUser, recordInto, sineWav } from './helpers'

test.use(fakeMic)

test('record, collaborate on audio marker, play lesson, import WAV', async ({ browser }) => {
  const alice = await newUser(browser, 'Rita')
  await newProject(alice, 'Audio test')
  await alice.getByRole('button', { name: 'Share', exact: true }).click()
  await alice.getByRole('button', { name: 'Create link' }).click()
  const link = await alice.getByRole('textbox', { name: 'Invite link', exact: true }).inputValue()
  await alice.keyboard.press('Escape')
  const bob = await newUser(browser, 'Rob')
  await bob.goto(link)
  await bob.getByRole('button', { name: 'Join project' }).click()

  const card = await addStudentLatin(alice, 'Salve')
  const dialog = await recordInto(alice, card)
  await expect(dialog.locator('.recorder__length')).toContainText(/\d/)
  await expect(dialog.locator('.recorder__length')).not.toHaveText(/^\s*$/)
  await dialog.getByRole('button', { name: 'Close', exact: true }).last().click()
  await expect(dialog).toHaveCount(0)

  await expect(bob.locator('.card[aria-label*="has audio"]')).toHaveCount(1)

  // Lesson summary shows a duration; the play button toggles.
  const header = alice.locator('.lesson-header').first()
  await expect(header.locator('.lesson-header__summary')).toContainText(/\d/)
  await header.getByRole('button', { name: 'Play lesson' }).click()
  await expect(header.getByRole('button', { name: /^(Pause|Resume|Play) lesson/ })).toBeVisible()
  await alice.waitForTimeout(500)

  // Import a WAV into a second entry.
  const second = await addStudentLatin(alice, 'Valē')
  await second.hover()
  await second.getByRole('button', { name: 'Record or import audio' }).click()
  const d2 = alice.getByRole('dialog', { name: 'Record audio' })
  await d2.locator('input[type=file]').setInputFiles({ name: 'tone.wav', mimeType: 'audio/wav', buffer: sineWav(1) })
  await expect(second).toHaveAttribute('aria-label', /has audio/, { timeout: 20_000 })
  await d2.getByRole('button', { name: 'Close', exact: true }).last().click()
})
