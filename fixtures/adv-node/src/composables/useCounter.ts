export const useCounter = (start = 0) => {
  let count = start
  return { inc: () => ++count, value: () => count }
}
