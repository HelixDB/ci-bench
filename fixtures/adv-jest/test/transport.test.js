jest.mock('../src/transport')

const { send } = require('../src/transport')

test('an automocked module returns undefined', () => {
  expect(send('x')).toBeUndefined()
})
