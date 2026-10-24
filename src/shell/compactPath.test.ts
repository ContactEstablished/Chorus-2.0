import { expect, it } from 'vitest'
import { compactPath } from './compactPath'

it('normalises short paths and preserves the last folder when shortening long paths', () => {
  expect(compactPath('C:\\Projects\\WeatherTest')).toBe('C:/Projects/WeatherTest')
  const result = compactPath('C:/Projects/Folder/SubFolder/AnotherSubfolder/ThisIsTooLong/ProjectName')
  expect(result.length).toBe(44)
  expect(result.startsWith('C:/Projects/')).toBe(true)
  expect(result.endsWith('.../ProjectName')).toBe(true)
})

it('bounds an oversized final folder without splitting Unicode characters', () => {
  const result = compactPath('C:/Projects/' + '🌤️'.repeat(40) + 'ProjectName')
  expect(Array.from(result).length).toBe(44)
  expect(result.endsWith('ProjectName')).toBe(true)
  expect(result).not.toMatch(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])/u)
})
