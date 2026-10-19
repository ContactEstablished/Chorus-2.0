import { describe, expect, it } from 'vitest'
import { Terminal } from '@xterm/xterm'
import { TERMINAL_OPTIONS } from './terminal'

// chorus_core::scrollback::restart_screen("── Restarted ──"); the Rust test
// the_restart_screen_pushes_the_screen_then_the_line_into_scrollback pins the same bytes.
const RESTART = '\x1b[?1049l\x1b[r\x1b[m\x1b[2J\x1b[H\x1b[2m── Restarted ──\x1b[0m\x1b[2J\x1b[H'
const LINE = '── Restarted ──'
const ROWS = 30

/** A screen the way ConPTY paints cmd.exe: the text, the remaining rows cleared, the cursor put back. */
function conptyScreen(lines: string[]): string {
  return lines.join('\r\n') + '\x1b[K\r\n'.repeat(ROWS - lines.length) + `\x1b[${lines.length};9H`
}

/** Replays output into a terminal with the app's options; returns every buffer line and where the viewport starts. */
async function replay(...chunks: string[]) {
  const term = new Terminal({ ...TERMINAL_OPTIONS, theme: { ...TERMINAL_OPTIONS.theme }, cols: 80, rows: ROWS })
  for (const chunk of chunks) await new Promise<void>((done) => term.write(chunk, done))
  const b = term.buffer.active
  const lines = Array.from({ length: b.length }, (_, i) => b.getLine(i)?.translateToString(true) ?? '')
  const result = { lines, viewportTop: b.baseY, buffer: b.type }
  term.dispose()
  return result
}

describe('the restart screen in a Chorus terminal', () => {
  const old = conptyScreen(['C:\\work>echo old', 'old', '', 'C:\\work>'])

  it('keeps the old screen directly above the dividing line, without its blank rows', async () => {
    const { lines, viewportTop } = await replay(old, RESTART, conptyScreen(['', 'C:\\work>']))
    expect(lines.filter((l) => l.trim())).toEqual(['C:\\work>echo old', 'old', 'C:\\work>', LINE, 'C:\\work>'])
    const at = lines.indexOf(LINE)
    expect(lines[at - 1]).toBe('C:\\work>')
    // The line is the last row of scrollback; the new run has the whole screen.
    expect(at).toBe(viewportTop - 1)
  })

  it('keeps the dividing line when the new run repaints from the top row', async () => {
    // Claude Code under ConPTY: a redraw starts with the cursor at the top-left, erasing row 1.
    const first = 'Claude Code v2\r\n> Try "fix lint errors"'
    const repaint = '\x1b[?25l\x1b[H\x1b[KClaude Code v2\x1b[K\r\n> Try "fix typecheck errors"\x1b[K'
    const { lines, viewportTop } = await replay(old, RESTART, first, repaint)
    expect(lines[viewportTop - 1]).toBe(LINE)
    expect(lines[viewportTop]).toBe('Claude Code v2')
  })

  it('leaves a full-screen program on the normal screen, where scrollback is', async () => {
    const { lines, viewportTop, buffer } = await replay('\x1b[?1049h\x1b[Hfull-screen frame', RESTART)
    expect(buffer).toBe('normal')
    expect(lines[viewportTop - 1]).toBe(LINE)
  })

  it('puts the line first when there was nothing to keep', async () => {
    const { lines, viewportTop } = await replay(RESTART)
    expect([lines[0], viewportTop]).toEqual([LINE, 1])
  })
})
