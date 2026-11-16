import { describe, expect, it } from 'vitest'
import type { FolderCheck } from '../../ipc'
import { asError, canRecheck, failure } from './folderCheck'

const ok: FolderCheck = { problem: null, git: null, gitNote: null, sharing: [], projectId: null }

describe('the folder check', () => {
  it("gives the back end's reason, or what went wrong reaching it", () => {
    expect(failure({ status: 'error', error: 'D:\gone is not a folder that exists.' }, 'the folder')).toBe('D:\gone is not a folder that exists.')
    expect(failure(new Error('IPC timed out'), 'the folder')).toBe('Could not check the folder: IPC timed out')
    expect(failure(new Error(' '), 'the folder')).toBe('Could not check the folder: Chorus did not answer.')
    expect(failure({ status: 'error', error: '' }, 'retained worktrees')).toBe('Could not check retained worktrees.')
  })
  it('keeps the reason of a rejection that was not an Error', () => {
    expect(asError('command folder_check not found').message).toBe('command folder_check not found')
    const e = new Error('kept')
    expect(asError(e)).toBe(e)
  })
  it('offers Check again only when a retry could help', () => {
    expect(canRecheck('C:\work', false, null, null)).toBe(true)
    expect(canRecheck('C:\work', false, null, 'Could not check the folder.')).toBe(true)
    expect(canRecheck('C:\work', false, { ...ok, problem: 'C:\work is not a folder that exists.' }, null)).toBe(true)
    expect(canRecheck('C:\work', false, { ...ok, gitNote: 'git did not answer.' }, null)).toBe(true)
    expect(canRecheck('C:\work', false, { ...ok }, 'Could not check retained worktrees.')).toBe(true)
    expect(canRecheck('C:\work', false, ok, null)).toBe(false)
    expect(canRecheck('C:\work', true, null, null)).toBe(false)
    expect(canRecheck('  ', false, null, null)).toBe(false)
  })
})
