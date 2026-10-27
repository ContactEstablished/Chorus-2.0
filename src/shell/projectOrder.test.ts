import { describe, expect, it } from 'vitest'
import { moveTo } from './projectOrder'

describe('moveTo', () => {
  it('moves a row down to the row it is dropped on', () => {
    expect(moveTo([1, 2, 3, 4], 1, 3)).toEqual([2, 3, 1, 4])
    expect(moveTo([1, 2, 3], 1, 2)).toEqual([2, 1, 3])
  })
  it('moves a row up to the row it is dropped on', () => {
    expect(moveTo([1, 2, 3, 4], 4, 2)).toEqual([1, 4, 2, 3])
  })
  it('leaves the order alone for a drop on itself or an unknown id', () => {
    expect(moveTo([1, 2, 3], 2, 2)).toEqual([1, 2, 3])
    expect(moveTo([1, 2, 3], 9, 2)).toEqual([1, 2, 3])
  })
})
