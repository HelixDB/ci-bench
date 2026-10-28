import { expect, it } from 'vitest'
import { useCounter } from '../src/composables/useCounter'

it('counts', () => {
  const counter = useCounter(1)
  counter.inc()
  expect(counter.value()).toBe(2)
})
