import MarkdownIt from 'markdown-it'

// Presentation only: never interpret these decorations as session state or an agent action.
const markdown = new MarkdownIt({ html: false, linkify: false, breaks: true, typographer: false })
const escape = markdown.utils.escapeHtml

/** Only web links leave the reader. Local paths and custom protocols remain readable text. */
export function webUrl(value: string): string | null {
  if (!/^https?:\/\//i.test(value) || /[\u0000-\u0020\u007f]/.test(value)) return null
  try {
    const url = new URL(value)
    return url.hostname && !url.username && !url.password ? url.href : null
  } catch { return null }
}
markdown.validateLink = (url) => webUrl(url) !== null

markdown.renderer.rules.link_open = (tokens, idx, options, _env, renderer) => {
  tokens[idx]!.attrSet('target', '_blank')
  tokens[idx]!.attrSet('rel', 'noopener noreferrer')
  tokens[idx]!.attrSet('title', `${tokens[idx]!.attrGet('href')} — opens in your browser`)
  return renderer.renderToken(tokens, idx, options)
}
markdown.renderer.rules.table_open = () => '<div class="message-table" tabindex="0" role="region" aria-label="Table"><table>\n'
markdown.renderer.rules.table_close = () => '</table></div>\n'

function codeCard(code: string, language: string): string {
  return `<section class="code-card"><header><span>${escape(language || 'Code')}</span><button type="button" data-message-action="copy-code" aria-label="Copy code">Copy code</button></header><pre tabindex="0"><code>${escape(code)}</code></pre></section>\n`
}
markdown.renderer.rules.fence = (tokens, idx) => codeCard(tokens[idx]!.content, tokens[idx]!.info.trim().split(/\s+/)[0] ?? '')
markdown.renderer.rules.code_block = (tokens, idx) => codeCard(tokens[idx]!.content, '')

// Images are explicit previews. No remote request is made just by reading an agent message.
markdown.renderer.rules.image = (tokens, idx, options, _env, renderer) => {
  const token = tokens[idx]!
  const alt = String(renderer.renderInlineAsText(token.children ?? [], options, {}) || 'Image')
  const src = webUrl(String(token.attrGet('src') ?? ''))
  if (!src || !src.startsWith('https://')) return `<span class="image-unavailable">${escape(alt)} · Image preview unavailable</span>`
  return `<span class="image-card" data-image-url="${escape(src)}" data-image-alt="${escape(alt)}"><span class="image-caption">${escape(alt)}</span><span class="image-host">${escape(new URL(src).hostname)}</span><button type="button" data-message-action="image">Show image</button><span class="image-status" role="status"></span></span>`
}

// Explicit highlight syntax is intentionally small; raw HTML/style injection stays disabled.
markdown.inline.ruler.before('emphasis', 'friendly_mark', (state, silent) => {
  const start = state.pos
  if (state.src.slice(start, start + 2) !== '==') return false
  const end = state.src.indexOf('==', start + 2)
  if (end < 0 || end >= state.posMax) return false
  const text = state.src.slice(start + 2, end)
  if (!text.trim() || text.includes('\n')) return false
  if (!silent) {
    state.push('mark_open', 'mark', 1)
    state.push('text', '', 0).content = text
    state.push('mark_close', 'mark', -1)
  }
  state.pos = end + 2
  return true
})

const CALLOUTS: Record<string, string> = { NOTE: 'Note', TIP: 'Tip', IMPORTANT: 'Important', WARNING: 'Warning', CAUTION: 'Caution' }
markdown.core.ruler.after('inline', 'friendly_blocks', (state) => {
  const tokens = state.tokens
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i]!
    const inline = tokens[i + 2]
    if (tokens[i + 1]?.type !== 'paragraph_open' || inline?.type !== 'inline') continue
    const first = inline.children?.[0]
    if (first?.type !== 'text') continue
    if (token.type === 'blockquote_open') {
      const match = /^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\](?:\s|$)/.exec(first.content)
      if (!match) continue
      const kind = match[1]!
      token.attrSet('class', `callout callout-${kind.toLowerCase()}`)
      token.meta = { callout: CALLOUTS[kind] }
      first.content = first.content.slice(match[0].length)
      if (!first.content && inline.children?.[1]?.type === 'softbreak') inline.children.splice(1, 1)
    } else if (token.type === 'list_item_open') {
      const match = /^\[([ xX])\]\s+/.exec(first.content)
      if (!match) continue
      const done = match[1]!.toLowerCase() === 'x'
      token.attrSet('class', `task-item${done ? ' task-done' : ''}`)
      token.meta = { task: true, done }
      first.content = first.content.slice(match[0].length)
    }
  }
})
markdown.renderer.rules.blockquote_open = (tokens, idx, options, _env, renderer) => {
  const title = tokens[idx]!.meta?.callout as string | undefined
  return renderer.renderToken(tokens, idx, options) + (title ? `<div class="callout-title">${escape(title)}</div>` : '')
}
markdown.renderer.rules.list_item_open = (tokens, idx, options, _env, renderer) => {
  const meta = tokens[idx]!.meta
  return renderer.renderToken(tokens, idx, options) + (meta?.task ? `<span class="task-marker" role="img" aria-label="${meta.done ? 'Completed' : 'Not completed'}">${meta.done ? '✓' : '○'}</span>` : '')
}

export function renderMessage(text: string): string {
  return markdown.render(text)
}
