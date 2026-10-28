import { Worker } from 'node:worker_threads'
import { expect, it } from 'vitest'

it('runs a worker thread', async () => {
  const worker = new Worker(new URL('../src/worker.mjs', import.meta.url))
  const answer = await new Promise((resolve, reject) => {
    worker.once('message', resolve)
    worker.once('error', reject)
  })
  await worker.terminate()
  expect(answer).toBe('pong 1')
})
