import { fileURLToPath } from 'node:url'
import vue from '@vitejs/plugin-vue'
import tsconfigPaths from 'vite-tsconfig-paths'
import { defineConfig } from 'vitest/config'
import { sharedPlugins, sharedTest } from './vitest.shared.ts'

export default defineConfig({
  plugins: [vue(), tsconfigPaths(), ...sharedPlugins],
  resolve: {
    alias: {
      '@lib': fileURLToPath(new URL('./src/lib', import.meta.url)),
    },
  },
  test: {
    include: ['test/**/*.test.ts'],
    setupFiles: ['./test/setup.ts'],
    ...sharedTest,
  },
})
