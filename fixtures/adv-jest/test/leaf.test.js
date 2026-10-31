const { leaf } = require('../src/leaf')

test('adds', () => {
  expect(leaf(2, 3)).toBe(5)
})
