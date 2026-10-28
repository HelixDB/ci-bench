import { expect, it } from 'vitest'
import { greet } from '@lib/greet'

it('resolves a vite alias', () => {
  expect(greet('adv')).toBe('hello, adv')
})
