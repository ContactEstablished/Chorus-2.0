/**
 * The xterm.js options every Chorus terminal uses.
 *
 * `scrollOnEraseInDisplay` matters to the back end: when a new run starts in a session, its
 * restart screen (`chorus_core::scrollback::restart_screen`) pushes the old run's screen and the
 * dividing line into scrollback with Erase in Display (`ESC[2J`). Without the option xterm.js
 * would erase them instead. With it, xterm.js moves the rows up to the last one with text into
 * scrollback, so the old screen's trailing blank rows aren't kept.
 */
import type { ITerminalOptions, ITheme } from '@xterm/xterm'

export const TERMINAL_OPTIONS = {
  fontFamily: "'JetBrains Mono', 'Cascadia Mono', Consolas, monospace",
  fontSize: 13,
  scrollback: 10_000,
  allowProposedApi: false,
  scrollOnEraseInDisplay: true,
  theme: { background: '#0a0f16', foreground: '#edf2f8', cursor: '#1ec8f0', selectionBackground: '#1ec8f055' },
} as const satisfies ITerminalOptions

/** The terminal colours of the theme on screen, from its tokens (the defaults if a token is missing). */
export function themeFromTokens(style: Pick<CSSStyleDeclaration, 'getPropertyValue'> = getComputedStyle(document.documentElement)): ITheme {
  const token = (name: string, fallback: string) => style.getPropertyValue(name).trim() || fallback
  const accent = token('--accent', TERMINAL_OPTIONS.theme.cursor)
  return {
    background: token('--bg', TERMINAL_OPTIONS.theme.background),
    foreground: token('--text', TERMINAL_OPTIONS.theme.foreground),
    cursor: accent,
    selectionBackground: `${accent}55`,
  }
}
