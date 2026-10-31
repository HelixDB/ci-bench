import { expect, it } from 'vitest'
import { createSSRApp, h } from 'vue'
import { renderToString } from 'vue/server-renderer'
import Badge from '../src/components/Badge.vue'

it('renders a single-file component', async () => {
  const html = await renderToString(createSSRApp({ render: () => h(Badge, { count: 3 }) }))
  expect(html).toBe('<span class="badge">3 items</span>')
})
