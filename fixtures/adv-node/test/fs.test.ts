import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { expect, it } from 'vitest'

it('reads a fixture file at run time', () => {
  const here = dirname(fileURLToPath(import.meta.url))
  const lines = readFileSync(join(here, 'fixtures', 'data.txt'), 'utf8').trim().split('\n')
  expect(lines).toEqual(['alpha', 'beta'])
})
