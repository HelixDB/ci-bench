import { shout } from '@adv/shared'
import { expect, it } from 'vitest'

it('imports a workspace package', () => {
  expect(shout('adv')).toBe('ADV!')
})
