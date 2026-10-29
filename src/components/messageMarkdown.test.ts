import { describe, expect, it } from 'vitest'
import { renderMessage, webUrl } from './messageMarkdown'

describe('Friendly message presentation', () => {
  it('renders the weather-stack reply as emphasis, a table, code and a web link', () => {
    const html = renderMessage('I’d use **plain HTML, CSS, and JavaScript**, with canned weather data. ([MDN](https://developer.mozilla.org/en-US/docs/Web))\n\n| Part | Choice |\n|---|---|\n| Interface | HTML and a small CSS stylesheet |\n| Project structure | `index.html`, `styles.css`, `app.js` |')
    expect(html).toContain('<strong>plain HTML, CSS, and JavaScript</strong>')
    expect(html).toContain('<th>Part</th>')
    expect(html).toContain('<td>HTML and a small CSS stylesheet</td>')
    expect(html).toContain('<code>index.html</code>')
    expect(html).toContain('href="https://developer.mozilla.org/en-US/docs/Web"')
    expect(html).toContain('rel="noopener noreferrer"')
  })

  it('keeps nested numbered lists and recognizes completed and incomplete tasks', () => {
    const html = renderMessage('3. Review\n   - [x] Build\n   - [ ] Check\n4. Ship')
    expect(html).toContain('<ol start="3">')
    expect(html).toContain('class="task-item task-done"')
    expect(html).toContain('aria-label="Completed"')
    expect(html).toContain('aria-label="Not completed"')
    expect(html).not.toContain('[x]')
    expect(html).toContain('Ship')
  })

  it('decorates explicit callouts and highlights without inventing meaning', () => {
    const html = renderMessage('> [!WARNING]\n> Keep **this file**.\n\n==Review this==\n\n> An ordinary quote.\n\nThis sentence mentions a warning.')
    expect(html).toContain('class="callout callout-warning"')
    expect(html).toContain('<div class="callout-title">Warning</div>')
    expect(html).toContain('<strong>this file</strong>')
    expect(html).toContain('<mark>Review this</mark>')
    expect(html).toContain('<blockquote>\n<p>An ordinary quote.')
    expect(html.match(/callout-title/g)).toHaveLength(1)
  })

  it('escapes HTML, fence labels and code; Markdown inside code stays literal', () => {
    const html = renderMessage('<img src=x onerror=alert(1)>\n\n```js"><img\nconst a = "<script>"; // **bold** ==mark==\n```')
    expect(html).not.toContain('<img')
    expect(html).not.toContain('<script>')
    expect(html).not.toContain('<strong>')
    expect(html).not.toContain('<mark>')
    expect(html).toContain('&lt;script&gt;')
    expect(html).toContain('data-message-action="copy-code"')
  })

  it('never makes script, data, file, credentialed or custom-protocol links clickable', () => {
    for (const url of ['javascript:alert%281%29', 'data:text/html,evil', 'file:///C:/secret', 'vscode://file/C:/secret', 'https://user:pass@example.com/', '//example.com', 'https://example.com/\npath']) {
      expect(webUrl(url)).toBeNull()
      expect(renderMessage(`[link](${url})`)).not.toContain('<a ')
    }
    expect(renderMessage('[bad](jav&#x61;script:alert%281%29)')).not.toContain('<a ')
    expect(webUrl('https://example.com/hello?q=one')).toBe('https://example.com/hello?q=one')
  })

  it('renders image placeholders without fetching a remote image or embedding active content', () => {
    const html = renderMessage('![Forecast](https://example.com/forecast.png)')
    expect(html).toContain('data-image-url="https://example.com/forecast.png"')
    expect(html).toContain('Show image')
    expect(html).not.toContain('<img')
    expect(html).not.toContain('<iframe')
    expect(renderMessage('![Insecure](http://example.com/a.png)')).toContain('Image preview unavailable')
    expect(renderMessage('![x](data:image/svg+xml,evil)')).not.toContain('data-image-url')
  })

  it('puts a URL that stands alone in a copyable code card', () => {
    const url = 'http://localhost:18080/enrollments/plan-detail?postalcode=98065&refnum=abc'
    for (const text of ['`' + url + '`', url]) {
      const html = renderMessage(`Here it is:

${text}

Done.`)
      expect(html).toContain('class="code-card"')
      expect(html).toContain('data-message-action="copy-code"')
      expect(html).toContain('postalcode=98065&amp;refnum=abc')
    }
  })

  it('leaves a URL inside a sentence inline', () => {
    expect(renderMessage('Open `http://localhost:1/x` now')).not.toContain('code-card')
  })
})
