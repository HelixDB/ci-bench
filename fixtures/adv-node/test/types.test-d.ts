// A type test: runs only when vitest's typecheck is enabled (scenario `typecheck`).
import { expectTypeOf, test } from 'vitest'
import { leaf } from '../src/leaf'

test('leaf returns a number', () => {
  expectTypeOf(leaf).returns.toEqualTypeOf<number>()
})
