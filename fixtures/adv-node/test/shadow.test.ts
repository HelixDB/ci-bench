import { expect, it } from 'vitest'
import { kind } from '../src/util'

it('imports a directory module', () => {
  expect(kind).toBe('directory')
})
