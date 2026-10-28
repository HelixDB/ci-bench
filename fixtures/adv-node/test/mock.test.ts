import { expect, it, vi } from 'vitest'
import { describeQuota } from '../src/quota'

vi.mock('../src/limits', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../src/limits')>()),
  UNIT: 'MB',
}))

it('reads a partially mocked module', () => {
  expect(describeQuota()).toBe('100 MB')
})
