import { describe, expect, it } from 'vitest'
import { extractImagePaths } from './imagePaths'

describe('extractImagePaths', () => {
  it('finds a backticked path, even with spaces', () => {
    expect(extractImagePaths('Screenshot: `C:\\Users\\matth\\AppData\\Local\\Temp\\shot-0.jpg` done')).toEqual(['C:\\Users\\matth\\AppData\\Local\\Temp\\shot-0.jpg'])
    expect(extractImagePaths('see `C:\\My Pics\\a.png`')).toEqual(['C:\\My Pics\\a.png'])
  })
  it('finds a bare path and drops trailing punctuation', () => {
    expect(extractImagePaths('Saved to C:/tmp/out.PNG.')).toEqual(['C:/tmp/out.PNG'])
  })
  it('ignores prose between a drive and a later image, non-images and traversal', () => {
    expect(extractImagePaths('C:\\ is the drive and then x.png')).toEqual([])
    expect(extractImagePaths('C:\\a\\notes.txt and C:\\a\\..\\b.png')).toEqual([])
  })
  it('lists each file once, in order', () => {
    expect(extractImagePaths('C:\\a.png then `C:\\b.png` then c:\\A.PNG')).toEqual(['C:\\a.png', 'C:\\b.png'])
  })
})
