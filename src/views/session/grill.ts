/**
 * Reads a "grill" round out of an agent's reply: numbered questions (`Q1 - Title: …`), each followed
 * by a recommended answer on a line that starts with ➡️ (or "Recommended:"). Pure: text in, questions out.
 */
export interface GrillQuestion {
  /** The question's number as the agent wrote it, such as `Q3`. */
  id: string
  title: string
  /** What the agent asked, as plain text. */
  body: string
  /** The agent's recommended answer, as plain text. */
  recommendation: string
}

const QUESTION = /^\s*(?:[❓?]️?\s*)?(?:\*\*|__)?(Q\d+)\b\s*[-–—:.)]?\s*(.*)$/
const RECOMMENDATION = /^\s*(?:(?:➡️?|→|👉)\s*|(?:\*\*|__)?Recommend(?:ed|ation)(?:\s+answer)?(?:\*\*|__)?\s*:\s*(?:\*\*|__)?)(.*)$/i
const BREAK = /^\s*(?:#{1,6}\s|-{3,}\s*$|\*{3,}\s*$)/

const plain = (s: string) => s.replace(/\*\*|__|`/g, '').replace(/\s+/g, ' ').trim()

/** Every question in `text` that has a recommendation; a question without one is left out. */
export function parseGrill(text: string | null | undefined): GrillQuestion[] {
  if (!text) return []
  const found: GrillQuestion[] = []
  let current: { id: string; head: string[]; rec: string[]; inRec: boolean } | null = null
  const close = () => {
    if (current && current.rec.length) {
      const head = plain(current.head.join(' '))
      const colon = head.indexOf(':')
      const hasTitle = colon > 0 && colon <= 90
      found.push({
        id: current.id,
        title: hasTitle ? head.slice(0, colon).trim() : head.slice(0, 60),
        body: hasTitle ? head.slice(colon + 1).trim() : head,
        recommendation: plain(current.rec.join(' ')),
      })
    }
    current = null
  }
  for (const line of text.split(/\r?\n/)) {
    if (BREAK.test(line)) { close(); continue }
    const q = QUESTION.exec(line)
    if (q) { close(); current = { id: q[1]!.toUpperCase(), head: [q[2]!], rec: [], inRec: false }; continue }
    if (!current) continue
    const r = RECOMMENDATION.exec(line)
    if (r) { current.rec.push(r[1]!); current.inRec = true; continue }
    if (!line.trim()) { if (current.inRec) close(); continue }
    if (current.inRec) current.rec.push(line)
    else current.head.push(line)
  }
  close()
  return found
}

export interface GrillAnswer { question: GrillQuestion; accept: boolean; text: string }

/** The reply to send: one line per answered question. Unanswered questions are left out. */
export function grillReply(answers: GrillAnswer[]): string {
  return answers
    .flatMap(({ question: q, accept, text }) => {
      const label = `${q.id} - ${q.title}`
      if (accept) return [`${label}: Accepted. ${q.recommendation}`]
      return text.trim() ? [`${label}: ${text.trim()}`] : []
    })
    .join('\n')
}
