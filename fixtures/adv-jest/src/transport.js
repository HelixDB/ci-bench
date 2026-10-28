// test/transport.test.js automocks this module (jest.mock without a factory).
module.exports = {
  send: (message) => `sent ${message}`,
}
