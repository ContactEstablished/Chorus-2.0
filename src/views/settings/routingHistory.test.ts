import { describe, expect, it, vi } from 'vitest'
import type { Result } from '../../ipc'
import type { RefreshLogEntry, RefreshLogView, SpeedHistoryRow } from '../../ipc/bindings'
import historyVue from './RoutingHistory.vue?raw'
import settingsVue from './RoutingSettings.vue?raw'
import {
  LOG_HELP,
  NO_MODEL,
  NO_REFRESHES,
  NO_SPEED,
  SPEED_HELP,
  logNote,
  logRows,
  settle,
  speedNote,
  speedRows,
} from './routingHistory'

const MIN = 60_000
const NOW = 1_700_000_000_000

function row(tag: string, over: Partial<SpeedHistoryRow> = {}): SpeedHistoryRow {
  return { tag, samples: 14, newestAtMs: NOW - 12 * MIN, newestStatus: 0, medianTps: 41.6, medianLatencyS: 0.4213, lowestUptime1d: 99.62, ...over }
}

function entry(over: Partial<RefreshLogEntry> = {}): RefreshLogEntry {
  return {
    atMs: NOW - 5_000,
    model: 'deepseek/deepseek-v4.1-flash',
    providerName: 'OpenRouter',
    error: null,
    endpointsTested: 6,
    spentUsd: 0.0123,
    estimateUsd: 0.05,
    ...over,
  }
}

describe('speedRows', () => {
  it('gives the tag, samples, age, speed, latency and lowest uptime', () => {
    expect(speedRows([row('novita/fp8')], NOW)).toEqual([
      { tag: 'novita/fp8', samples: 14, newest: '12 min ago', speed: '42 tok/s', latency: '0.42 s', uptime: '99.62%', status: null },
    ])
  })

  it('flags "status 2" only when the newest status is 2', () => {
    expect(speedRows([row('a', { newestStatus: 2 })], NOW)[0]?.status).toBe('status 2')
    expect(speedRows([row('a', { newestStatus: 0 })], NOW)[0]?.status).toBeNull()
  })

  it('flags any status but 0, and leaves an unknown status unflagged', () => {
    expect(speedRows([row('a', { newestStatus: 429 })], NOW)[0]?.status).toBe('status 429')
    expect(speedRows([row('a', { newestStatus: null })], NOW)[0]?.status).toBeNull()
  })

  it('shows "—" for a missing speed, latency and uptime, never zero', () => {
    const [r] = speedRows([row('a', { medianTps: null, medianLatencyS: null, lowestUptime1d: null })], NOW)
    expect(r).toMatchObject({ speed: '—', latency: '—', uptime: '—' })
  })

  it('shows "—" for a newest sample with no time', () => {
    expect(speedRows([row('a', { newestAtMs: null })], NOW)[0]?.newest).toBe('—')
  })

  it('counts a sample from the last minute as just now', () => {
    expect(speedRows([row('a', { newestAtMs: NOW - 5_000 })], NOW)[0]?.newest).toBe('just now')
  })

  it('keeps Rust’s order (the ranking’s candidates, then the rest by name) and gives nothing for no rows', () => {
    expect(speedRows([row('z/b'), row('a/c'), row('m/d')], NOW).map((r) => r.tag)).toEqual(['z/b', 'a/c', 'm/d'])
    expect(speedRows([], NOW)).toEqual([])
  })

  it('gives one sample as 1', () => {
    expect(speedRows([row('a', { samples: 1 })], NOW)[0]?.samples).toBe(1)
  })
})

describe('logRows', () => {
  it('gives when, model, provider, Done, endpoints tested, and the spend and estimate', () => {
    expect(logRows([entry()], NOW)).toEqual([
      {
        when: 'just now',
        model: 'deepseek/deepseek-v4.1-flash',
        provider: 'OpenRouter',
        result: 'Done',
        failed: false,
        endpoints: 6,
        spent: '$0.0123',
        estimate: '$0.0500',
      },
    ])
  })

  it('gives the error’s message for a failed Refresh, with the zero figures Rust stored', () => {
    const [r] = logRows([entry({ error: 'OpenRouter refused the key.', endpointsTested: 0, spentUsd: 0, estimateUsd: 0 })], NOW)
    expect(r).toMatchObject({ result: 'OpenRouter refused the key.', failed: true, endpoints: 0, spent: '$0.0000', estimate: '$0.0000' })
  })

  it('says why when a failure has no message', () => {
    expect(logRows([entry({ error: '  ' })], NOW)[0]).toMatchObject({ result: 'Failed: no reason was given.', failed: true })
  })

  it('shows "—" for an unknown time, spend or estimate, never $0', () => {
    const [r] = logRows([entry({ atMs: null, spentUsd: null, estimateUsd: null })], NOW)
    expect(r).toMatchObject({ when: '—', spent: '—', estimate: '—' })
  })

  it('writes money under $1 with four decimals and otherwise two', () => {
    const [r] = logRows([entry({ spentUsd: 1.234, estimateUsd: 0.0005 })], NOW)
    expect(r).toMatchObject({ spent: '$1.23', estimate: '$0.0005' })
  })

  it('dates each entry relative to now, and keeps the newest-first order it is given', () => {
    const rows = logRows([entry({ atMs: NOW - 2 * MIN, model: 'a/x' }), entry({ atMs: NOW - 3 * 60 * MIN, model: 'b/y' })], NOW)
    expect(rows.map((r) => [r.model, r.when])).toEqual([
      ['a/x', '2 min ago'],
      ['b/y', '3 h ago'],
    ])
    expect(logRows([], NOW)).toEqual([])
  })
})

describe('the texts', () => {
  it('has decisions.md’s speed history help, exactly', () => {
    expect(SPEED_HELP).toBe("From the endpoint numbers stored for this model, up to the history setting's age.")
  })

  it('has the three empty states, exactly', () => {
    expect(NO_MODEL).toBe('Inspect a model to see its speed history.')
    expect(NO_SPEED).toBe('No speed history for this model yet.')
    expect(NO_REFRESHES).toBe('No Refreshes yet.')
  })

  it('says what the Refresh log holds', () => {
    expect(LOG_HELP).toBe('The last 50 Refreshes, for every model, newest first. A failed Refresh is listed with its message.')
  })
})

describe('speedNote', () => {
  it('asks for a model to inspect when there is none, even before anything loads', () => {
    expect(speedNote('', null)).toBe(NO_MODEL)
    expect(speedNote('   ', [row('a')])).toBe(NO_MODEL)
  })

  it('says there is no history for a model with no rows', () => {
    expect(speedNote('a/x', [])).toBe(NO_SPEED)
  })

  it('says nothing when there are rows, or before they have loaded', () => {
    expect(speedNote('a/x', [row('a')])).toBeNull()
    expect(speedNote('a/x', null)).toBeNull()
  })
})

describe('logNote', () => {
  const view = (over: Partial<RefreshLogView> = {}): RefreshLogView => ({ entries: [], problem: null, ...over })

  it('says "No Refreshes yet." for an empty log', () => {
    expect(logNote(view())).toBe('No Refreshes yet.')
  })

  it('shows the problem as Rust words it, when the stored log can’t be read', () => {
    expect(logNote(view({ problem: "The Refresh log couldn't be read." }))).toBe("The Refresh log couldn't be read.")
  })

  it('says nothing when there are entries, or before the log has loaded', () => {
    expect(logNote(view({ entries: [entry()] }))).toBeNull()
    expect(logNote(null)).toBeNull()
  })

  it('shows the problem and not "No Refreshes yet." for an unreadable log, which has no entries', () => {
    expect(logNote(view({ problem: 'x' }))).not.toBe(NO_REFRESHES)
  })
})

describe('settle', () => {
  const ok = <T>(data: T): Result<T> => ({ status: 'ok', data })

  it('returns the data of an answer', async () => {
    expect(await settle(async () => ok([row('a')]))).toEqual({ ok: true, data: [row('a')] })
  })

  it('returns Rust’s reason for a refusal, such as an Anthropic model', async () => {
    const reason = "Chorus doesn't route Anthropic or OpenAI models through OpenRouter. Run them in Claude Code or Codex on your own sign-in."
    expect(await settle(async (): Promise<Result<SpeedHistoryRow[]>> => ({ status: 'error', error: reason }))).toEqual({ ok: false, reason })
  })

  it('reports a call that throws', async () => {
    const boom = vi.fn(async (): Promise<Result<null>> => {
      throw new Error('ipc closed')
    })
    expect(await settle(boom)).toEqual({ ok: false, reason: 'Error: ipc closed' })
  })
})

describe('the section renders, and the logic stays in the module', () => {
  it('RoutingHistory.vue takes its words and rows from routingHistory.ts and defines none of them', () => {
    expect(historyVue).toMatch(/from '\.\/routingHistory'/)
    for (const name of ['speedRows', 'logRows', 'speedNote', 'logNote', 'settle']) {
      expect(historyVue).toContain(name)
      expect(historyVue).not.toMatch(new RegExp(`function ${name}\\(`))
    }
    expect(historyVue).toContain('{{ SPEED_HELP }}')
    expect(historyVue).toContain('{{ LOG_HELP }}')
    expect(historyVue).toContain('{{ speedText }}')
    expect(historyVue).toContain('{{ logText }}')
    // The texts come from the module, not a copy.
    expect(historyVue).not.toContain('From the endpoint numbers')
    expect(historyVue).not.toContain('Inspect a model to see')
    expect(historyVue).not.toContain('No speed history')
    expect(historyVue).not.toContain('No Refreshes yet')
  })

  it('reads the two stored histories through their commands, and makes no request of its own', () => {
    expect(historyVue).toContain('commands.routingHistory(model)')
    expect(historyVue).toContain('commands.routingRefreshLog()')
    for (const name of ['routingRefresh(', 'routingWatch', 'routingSettingsSave', 'routingSettingsReset', 'routingObserveModel'])
      expect(historyVue).not.toContain(name)
  })

  it('takes the inspected model and a version, and reloads when either changes', () => {
    expect(historyVue).toMatch(/defineProps<\{ model: string; version: number \}>/)
    expect(historyVue).toMatch(/watch\(\s*\(\) => props\.model/)
    expect(historyVue).toMatch(/watch\(\s*\(\) => props\.version/)
  })

  it('shows a refusal or a failed read as the reason, and ignores an answer for an earlier model', () => {
    expect(historyVue).toContain('{{ speedError }}')
    expect(historyVue).toMatch(/if \(mine !== speedSeq\) return/)
  })

  it('stops its clock when it goes', () => {
    expect(historyVue).toMatch(/onUnmounted\(\(\) => clearInterval\(ticker\)\)/)
  })
})

describe('RoutingSettings.vue connects it', () => {
  it('shows it last, after the observation', () => {
    expect(settingsVue).toMatch(/import RoutingHistory from '\.\/RoutingHistory\.vue'/)
    expect(settingsVue.indexOf('<RoutingObservation')).toBeLessThan(settingsVue.indexOf('<RoutingHistory'))
  })

  it('follows the inspected model, taken from the inspector’s `inspect` event', () => {
    expect(settingsVue).toMatch(/<RoutingInspect [^>]*@inspect="model = \$event\.model"/)
    expect(settingsVue).toMatch(/<RoutingHistory :model="model" /)
  })

  it('reads again after each Refresh, and after the ranking settings are saved', () => {
    // Refresh bumps its own version, so the inspector (which reloads on `inspectVersion`) doesn't load twice.
    expect(settingsVue).toMatch(/<RoutingInspect [^>]*@refreshed="refreshVersion\+\+"/)
    expect(settingsVue).toContain('<RoutingHistory :model="model" :version="inspectVersion + refreshVersion" />')
    // A saved age decides which samples count, so a Save changes the speed history too.
    expect(settingsVue).toContain('<RoutingRanking @saved="inspectVersion++" />')
  })

  it('keeps the inspector reloading on `inspectVersion` only, as T10 pinned', () => {
    expect(settingsVue).toMatch(/<RoutingInspect :version="inspectVersion"( @[a-z]+="[^"]*")* \/>/)
    expect(settingsVue).not.toMatch(/<RoutingInspect [^>]*refreshVersion"/)
  })
})
