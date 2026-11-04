import { describe, expect, it } from 'vitest'
import { gridShape } from './gridShape'

describe('gridShape', () => {
  it('gives one session the whole box', () => {
    expect(gridShape(1, 1200, 800, 12)).toEqual({ cols: 1, rows: 1, scroll: false })
  })
  it('puts two sessions side by side in a wide box and stacked in a tall one', () => {
    expect(gridShape(2, 1600, 600, 12)).toEqual({ cols: 2, rows: 1, scroll: false })
    expect(gridShape(2, 700, 1000, 12)).toEqual({ cols: 1, rows: 2, scroll: false })
  })
  it('fills a wide box with four sessions as two by two', () => {
    expect(gridShape(4, 1400, 800, 12)).toEqual({ cols: 2, rows: 2, scroll: false })
  })
  it('never leaves a whole row empty', () => {
    for (let n = 1; n <= 12; n++) {
      const { cols, rows } = gridShape(n, 1400, 900, 12)
      expect(cols * (rows - 1)).toBeLessThan(n)
    }
  })
  it('scrolls when the tiles would be too small to read', () => {
    expect(gridShape(30, 1000, 600, 12)).toEqual({ cols: 3, rows: 10, scroll: true })
  })
  it('copes with nothing to show or no room yet', () => {
    expect(gridShape(0, 1000, 600, 12).scroll).toBe(false)
    expect(gridShape(3, 0, 0, 12).scroll).toBe(false)
  })
})
