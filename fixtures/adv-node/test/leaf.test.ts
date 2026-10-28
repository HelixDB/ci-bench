import { expect, it } from 'vitest'
import { leaf } from '../src/leaf'

it('adds', () => {
  expect(leaf(2, 3)).toBe(5)
})
