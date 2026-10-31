export const price = (cents: number): string => `€${(cents / 100).toFixed(2)}`
