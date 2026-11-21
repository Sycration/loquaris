import { expect, test } from '@playwright/test'
import { newProject, newUser } from './helpers'

test('two people edit one project at the same time', async ({ browser }) => {
  const alice = await newUser(browser, 'Alice')
  await newProject(alice, 'Via Latina')
  const projectUrl = alice.url()

  // Share an editor invite link with Bob.
  await alice.getByRole('button', { name: 'Share', exact: true }).click()
  await alice.getByRole('button', { name: 'Create link' }).click()
  const link = await alice.getByRole('textbox', { name: 'Invite link', exact: true }).inputValue()
  expect(link).toContain('/invite/')
  await alice.keyboard.press('Escape')

  const bob = await newUser(browser, 'Bob')
  await bob.goto(link)
  await bob.getByRole('button', { name: 'Join project' }).click()
  await expect(bob).toHaveURL(projectUrl)

  // Each adds a line; both see both.
  const addLatin = (page: typeof alice) => page.locator('.add-bar__side--student').getByRole('button', { name: '+ Latin' })
  await addLatin(alice).click()
  await alice.keyboard.type('Salvē, discipule!')
  await alice.keyboard.press('Escape')
  await expect(bob.getByText('Salvē, discipule!')).toBeVisible()

  await addLatin(bob).click()
  await bob.keyboard.type('Salvē, magister!')
  await bob.keyboard.press('Escape')
  await expect(alice.getByText('Salvē, magister!')).toBeVisible()

  // Presence: each sees the other in the header.
  await expect(alice.getByRole('banner').getByLabel(/Also here: Bob/)).toBeVisible()
  await expect(bob.getByRole('banner').getByLabel(/Also here: Alice/)).toBeVisible()

  // Changes persist across a reload.
  await bob.reload()
  await expect(bob.getByText('Salvē, discipule!')).toBeVisible()
  await expect(bob.getByText('Salvē, magister!')).toBeVisible()
})
