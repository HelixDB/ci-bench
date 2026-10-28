import { execFileSync } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { expect, it } from 'vitest'

it("runs another workspace package's bin", () => {
  const root = join(dirname(fileURLToPath(import.meta.url)), '..')
  const bin = join(root, 'packages', 'cli', 'bin', 'adv-cli.mjs')
  expect(execFileSync(process.execPath, [bin], { encoding: 'utf8' }).trim()).toBe('adv-cli 1')
})
