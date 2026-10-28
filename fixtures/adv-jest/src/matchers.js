// Custom matchers, registered by test/setup.js (setupFilesAfterEnv) only.
module.exports = {
  toBeEven(received) {
    const pass = received % 2 === 0
    return { pass, message: () => `expected ${received} ${pass ? 'not ' : ''}to be even` }
  },
}
