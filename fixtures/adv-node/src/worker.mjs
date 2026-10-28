// Run by test/worker.test.ts as a worker thread (`new Worker(new URL(...))`).
import { parentPort } from 'node:worker_threads'

parentPort.postMessage('pong 1')
