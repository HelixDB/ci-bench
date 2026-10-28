import { expect, it } from 'vitest'

it('sees the region the setup file set', () => {
  expect((globalThis as { __ADV_REGION__?: string }).__ADV_REGION__).toBe('eu')
})
