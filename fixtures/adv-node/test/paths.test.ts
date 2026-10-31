import { expect, it } from 'vitest'
import { slug } from '~paths/slug'

it('resolves a tsconfig path', () => {
  expect(slug('Hello World')).toBe('hello-world')
})
