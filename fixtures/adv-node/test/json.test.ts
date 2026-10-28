import { expect, it } from 'vitest'
import config from '../src/data/config.json'

it('imports json', () => {
  expect(config.retries).toBe(3)
})
