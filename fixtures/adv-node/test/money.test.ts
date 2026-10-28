import { expect, it } from 'vitest'
import { price } from '../src/money'

it('formats a price', () => {
  expect(price(1250)).toBe('€12.50')
})
