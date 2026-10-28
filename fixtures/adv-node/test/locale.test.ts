import { expect, it } from 'vitest'

it('loads a locale through a template import()', async () => {
  const lang = 'en'
  const { messages } = await import(`../src/locales/${lang}.ts`)
  expect(messages.hello).toBe('Hello')
})
