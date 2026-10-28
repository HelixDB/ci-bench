const defaults = require('@cfg/defaults')

test('resolves a moduleNameMapper path', () => {
  expect(defaults.retries).toBe(3)
})
