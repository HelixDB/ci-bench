import { epoch } from '#internal/clock'
import { expect, it } from 'vitest'

it('resolves a package.json imports subpath', () => {
  expect(epoch()).toBe(0)
})
