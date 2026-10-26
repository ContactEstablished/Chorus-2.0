import { describe, expect, it, vi } from 'vitest'
import type { RoutingSettings, RoutingSettingsView } from '../../ipc/bindings'
import rankingVue from './RoutingRanking.vue?raw'
import settingsVue from './RoutingSettings.vue?raw'
import {
  DATA_COLLECTION_HELP,
  DATA_COLLECTION_LABEL,
  KEEP_HISTORY_HELP,
  NITRO_NOTE,
  POLICY_CHOICES,
  RANKING_FIELDS,
  RANKING_GROUPS,
  RESET_NOTICE,
  RESTORE_TITLE,
  SAVED_NOTICE,
  actionStates,
  defaultText,
  fromDraft,
  isDirty,
  refusalText,
  saveDataCollection,
  saveDraft,
  settle,
  switchBlocked,
  toDraft,
  withDataCollection,
  type Draft,
} from './routingRanking'

// `RoutingSettings::default()` in chorus-core, which the view's `defaults` carry at run time.
const DEFAULTS: RoutingSettings = {
  minUptimePct: 99.5,
  readmitUptimePct: 99.6,
  outageGuard5mPct: 95,
  unknownQuantPolicy: 'first-party-and-verified',
  smoothingWindow: 6,
  stableMinObservations: 3,
  observationMaxAgeDays: 7,
  snapshotMaxAgeMinutes: 60,
  budgetMinTps: 30,
  budgetMedianFraction: 0.5,
  tierWeights: { budget: 0.3, balanced: 0.5, fast: 1 },
  tieRatio: 1.01,
  fallbackCount: 2,
  dataCollection: 'deny',
}

function view(over: Partial<RoutingSettingsView> = {}): RoutingSettingsView {
  return { settings: DEFAULTS, defaults: DEFAULTS, stored: false, problem: null, ...over }
}

const label = (key: string) => RANKING_FIELDS.find((f) => f.key === key)!.label

describe('RANKING_FIELDS', () => {
  it('lists the 15 fields other than dataCollection, once each, in decisions.md’s order', () => {
    expect(RANKING_FIELDS.map((f) => f.key)).toEqual([
      'minUptimePct',
      'readmitUptimePct',
      'outageGuard5mPct',
      'unknownQuantPolicy',
      'smoothingWindow',
      'stableMinObservations',
      'observationMaxAgeDays',
      'snapshotMaxAgeMinutes',
      'tieRatio',
      'fallbackCount',
      'tierWeights.budget',
      'budgetMinTps',
      'budgetMedianFraction',
      'tierWeights.balanced',
      'tierWeights.fast',
    ])
  })

  it('covers every RoutingSettings field but dataCollection', () => {
    // tierWeights is three fields, tierWeights.budget, .balanced and .fast.
    const settingsKeys = Object.keys(DEFAULTS)
      .filter((k) => k !== 'dataCollection')
      .flatMap((k) => (k === 'tierWeights' ? Object.keys(DEFAULTS.tierWeights).map((w) => `tierWeights.${w}`) : [k]))
    expect(RANKING_FIELDS.map((f) => f.key).sort()).toEqual(settingsKeys.sort())
    expect(RANKING_FIELDS.map((f) => f.key)).not.toContain('dataCollection')
  })

  it('has decisions.md’s labels', () => {
    expect(RANKING_FIELDS.map((f) => f.label)).toEqual([
      'Minimum uptime over a day',
      'Uptime to come back after a dip',
      'Minimum uptime over the last 5 minutes',
      "Endpoints that don't state their precision",
      'Speed samples averaged',
      'Samples before speed history counts',
      'Keep speed history for',
      'A ranking needs a Refresh after',
      'Scores this close count as a tie',
      'Fallback endpoints after the first',
      'Budget: weight on speed, the rest is price',
      'Budget: speed floor',
      'Budget: or this share of the median speed, whichever is higher',
      'Balanced: weight on speed',
      'Fast: weight on speed',
    ])
  })

  it('groups them as Every tier, Budget, Balanced and Fast, and Nitro has none', () => {
    expect(RANKING_GROUPS.map((g) => [g.key, g.heading, g.fields.length])).toEqual([
      ['every', 'Every tier', 10],
      ['budget', 'Budget', 3],
      ['balanced', 'Balanced', 1],
      ['fast', 'Fast', 1],
      ['nitro', 'Nitro', 0],
    ])
    expect(RANKING_GROUPS.flatMap((g) => g.fields.map((f) => f.key))).toEqual(RANKING_FIELDS.map((f) => f.key))
    expect(RANKING_GROUPS.every((g) => g.fields.every((f) => f.group === g.key))).toBe(true)
  })

  it('shows each tier’s fields without the tier’s name, and the others as labelled', () => {
    for (const f of RANKING_FIELDS) {
      const prefixed = /^(Budget|Balanced|Fast): (.*)$/.exec(f.label)
      const expected = prefixed ? prefixed[2]!.charAt(0).toUpperCase() + prefixed[2]!.slice(1) : f.label
      expect(f.short).toBe(expected)
      expect(f.short).not.toMatch(/^(Budget|Balanced|Fast): /)
    }
    expect(label('tierWeights.fast')).toBe('Fast: weight on speed')
    expect(RANKING_FIELDS.find((f) => f.key === 'tierWeights.fast')!.short).toBe('Weight on speed')
  })

  it('marks only the three counts as whole numbers', () => {
    expect(RANKING_FIELDS.filter((f) => f.whole).map((f) => f.key)).toEqual(['smoothingWindow', 'stableMinObservations', 'fallbackCount'])
  })

  it('gives the units that the defaults show', () => {
    const units = Object.fromEntries(RANKING_FIELDS.map((f) => [f.key, f.unit]))
    expect(units).toMatchObject({
      minUptimePct: '%',
      readmitUptimePct: '%',
      outageGuard5mPct: '%',
      observationMaxAgeDays: 'days',
      snapshotMaxAgeMinutes: 'min',
      budgetMinTps: 'tok/s',
    })
  })

  it('has the Keep-speed-history help on its own field only', () => {
    expect(RANKING_FIELDS.filter((f) => f.help).map((f) => [f.key, f.help])).toEqual([['observationMaxAgeDays', KEEP_HISTORY_HELP]])
  })
})

describe('defaultText', () => {
  it('gives decisions.md’s bracketed defaults, in field order', () => {
    expect(RANKING_FIELDS.map(defaultText)).toEqual([
      '99.5%',
      '99.6%',
      '95%',
      "Only the model's maker at full precision, or ones verified",
      '6',
      '3',
      '7 days',
      '60 min',
      '1.01',
      '2',
      '0.3',
      '30 tok/s',
      '0.5',
      '0.5',
      '1.0',
    ])
  })

  it('agrees with the defaults Rust sends beside the settings', () => {
    const draft = toDraft(DEFAULTS)
    for (const f of RANKING_FIELDS) {
      if (f.key === 'unknownQuantPolicy') continue
      expect(parseFloat(defaultText(f)), f.key).toBe(Number(draft[f.key]))
      // A unit in the default text is the field's unit.
      expect(defaultText(f).endsWith(f.unit), f.key).toBe(true)
    }
    const policy = POLICY_CHOICES.find((c) => c.value === DEFAULTS.unknownQuantPolicy)!
    expect(defaultText(RANKING_FIELDS.find((f) => f.key === 'unknownQuantPolicy')!)).toBe(policy.label)
  })

  it('offers the two policies, the default first', () => {
    expect(POLICY_CHOICES).toEqual([
      { value: 'first-party-and-verified', label: "Only the model's maker at full precision, or ones verified" },
      { value: 'strict', label: 'Never' },
    ])
  })
})

describe('toDraft and fromDraft', () => {
  it('turns every field into text, and back into the same settings', () => {
    const draft = toDraft(DEFAULTS)
    expect(draft).toMatchObject({ minUptimePct: '99.5', smoothingWindow: '6', 'tierWeights.fast': '1', unknownQuantPolicy: 'first-party-and-verified' })
    expect(Object.keys(draft)).toHaveLength(15)
    expect(fromDraft(draft, DEFAULTS)).toEqual({ ok: true, settings: DEFAULTS })
  })

  it('keeps the saved dataCollection, which the draft doesn’t hold', () => {
    const saved: RoutingSettings = { ...DEFAULTS, dataCollection: 'allow', fallbackCount: 4 }
    const r = fromDraft(toDraft(DEFAULTS), saved)
    expect(r).toEqual({ ok: true, settings: { ...DEFAULTS, dataCollection: 'allow' } })
  })

  it('reads edits, with spaces around them', () => {
    const draft: Draft = { ...toDraft(DEFAULTS), fallbackCount: ' 1 ', 'tierWeights.budget': '.25', budgetMinTps: '45.5', unknownQuantPolicy: 'strict' }
    const r = fromDraft(draft, DEFAULTS)
    expect(r).toEqual({
      ok: true,
      settings: { ...DEFAULTS, fallbackCount: 1, tierWeights: { ...DEFAULTS.tierWeights, budget: 0.25 }, budgetMinTps: 45.5, unknownQuantPolicy: 'strict' },
    })
  })

  it('refuses a value that isn’t a number, naming the field', () => {
    const draft = { ...toDraft(DEFAULTS), smoothingWindow: 'abc' }
    expect(fromDraft(draft, DEFAULTS)).toEqual({ ok: false, error: '“Speed samples averaged” needs a number.' })
  })

  it('refuses what only looks like a number', () => {
    for (const text of ['', '   ', '-', '.', '1e3', '0x10', 'Infinity', 'NaN', '1,5', '5 %', '9'.repeat(400)]) {
      const r = fromDraft({ ...toDraft(DEFAULTS), tieRatio: text }, DEFAULTS)
      expect(r, JSON.stringify(text)).toEqual({ ok: false, error: '“Scores this close count as a tie” needs a number.' })
    }
  })

  it('refuses a fractional value for a count, naming the field', () => {
    const draft = { ...toDraft(DEFAULTS), fallbackCount: '2.5' }
    expect(fromDraft(draft, DEFAULTS)).toEqual({ ok: false, error: '“Fallback endpoints after the first” needs a whole number.' })
  })

  it('refuses a negative or oversized count as not whole, and lets a decimal field be fractional', () => {
    for (const text of ['-1', '4294967296']) {
      const r = fromDraft({ ...toDraft(DEFAULTS), stableMinObservations: text }, DEFAULTS)
      expect(r, text).toEqual({ ok: false, error: '“Samples before speed history counts” needs a whole number.' })
    }
    expect(fromDraft({ ...toDraft(DEFAULTS), fallbackCount: '0', observationMaxAgeDays: '0.5' }, DEFAULTS).ok).toBe(true)
    expect(fromDraft({ ...toDraft(DEFAULTS), fallbackCount: '5.' }, DEFAULTS).ok).toBe(true)
  })

  it('leaves negative decimals to Rust’s own rules', () => {
    const r = fromDraft({ ...toDraft(DEFAULTS), budgetMinTps: '-5' }, DEFAULTS)
    expect(r).toEqual({ ok: true, settings: { ...DEFAULTS, budgetMinTps: -5 } })
  })

  it('names the first field, in the list’s order, that fails', () => {
    const draft = { ...toDraft(DEFAULTS), fallbackCount: 'x', minUptimePct: 'y' }
    expect(fromDraft(draft, DEFAULTS)).toEqual({ ok: false, error: '“Minimum uptime over a day” needs a number.' })
  })

  it('refuses a policy that isn’t one of the two choices', () => {
    const r = fromDraft({ ...toDraft(DEFAULTS), unknownQuantPolicy: 'lenient' }, DEFAULTS)
    expect(r).toEqual({ ok: false, error: "“Endpoints that don't state their precision” needs one of its choices." })
  })

  it('shows an unreadable number as an empty input, which Save then refuses', () => {
    const odd = { ...DEFAULTS, tieRatio: null } as unknown as RoutingSettings
    expect(toDraft(odd).tieRatio).toBe('')
    expect(fromDraft(toDraft(odd), DEFAULTS).ok).toBe(false)
  })

  it('knows whether the inputs differ from the saved settings', () => {
    expect(isDirty(toDraft(DEFAULTS), DEFAULTS)).toBe(false)
    expect(isDirty({ ...toDraft(DEFAULTS), fallbackCount: '1' }, DEFAULTS)).toBe(true)
    expect(isDirty({ ...toDraft(DEFAULTS), 'tierWeights.fast': '0.9' }, DEFAULTS)).toBe(true)
    expect(isDirty({ ...toDraft(DEFAULTS), unknownQuantPolicy: 'strict' }, DEFAULTS)).toBe(true)
    // Typing back what was saved is no change.
    expect(isDirty({ ...toDraft(DEFAULTS), fallbackCount: '1' }, { ...DEFAULTS, fallbackCount: 1 })).toBe(false)
  })
})

describe('refusalText', () => {
  it('names the field by its label', () => {
    expect(refusalText('fallbackCount must be at most 5')).toBe('“Fallback endpoints after the first” must be at most 5')
  })

  it('names both fields in a message about two, and neither camelCase name', () => {
    const text = refusalText('readmitUptimePct below minUptimePct')
    expect(text).toBe('“Uptime to come back after a dip” below “Minimum uptime over a day”')
    expect(text).not.toContain('readmitUptimePct')
    expect(text).not.toContain('minUptimePct')
    expect(refusalText('stableMinObservations exceeds smoothingWindow')).toBe('“Samples before speed history counts” exceeds “Speed samples averaged”')
  })

  it('names the dotted weights and every message Rust’s validate can give', () => {
    expect(refusalText('tierWeights.budget must be between 0 and 1')).toBe('“Budget: weight on speed, the rest is price” must be between 0 and 1')
    expect(refusalText('tierWeights.balanced must be between 0 and 1')).toBe('“Balanced: weight on speed” must be between 0 and 1')
    expect(refusalText('tierWeights.fast must be between 0 and 1')).toBe('“Fast: weight on speed” must be between 0 and 1')
    const messages = [
      'minUptimePct must be between 0 and 100',
      'readmitUptimePct must be between 0 and 100',
      'outageGuard5mPct must be between 0 and 100',
      'smoothingWindow must be at least 1',
      'stableMinObservations must be at least 1',
      'observationMaxAgeDays must be positive',
      'snapshotMaxAgeMinutes must be positive',
      'budgetMinTps must not be negative',
      'budgetMedianFraction must be between 0 and 1',
      'tieRatio must be at least 1',
      'smoothingWindow must be at most 50',
      'observationMaxAgeDays must be at most 30',
      'snapshotMaxAgeMinutes must be at most 1440',
      'fallbackCount must be at most 5',
      'tieRatio must be at most 2',
      'budgetMinTps must be at most 10000',
    ]
    for (const m of messages) {
      const text = refusalText(m)
      const name = m.split(' ')[0]!
      expect(text, m).toMatch(/^“[^”]+” /)
      expect(text, m).not.toContain(name)
      expect(text, m).toBe(`“${label(name)}”${m.slice(name.length)}`)
    }
  })

  it('returns a message that names no field as it is', () => {
    expect(refusalText('The database is locked.')).toBe('The database is locked.')
    expect(refusalText('')).toBe('')
    // A word that merely contains a field name is not the field.
    expect(refusalText('myfallbackCount is odd')).toBe('myfallbackCount is odd')
  })
})

describe('withDataCollection and switchBlocked', () => {
  it('changes only dataCollection', () => {
    const on = withDataCollection(DEFAULTS, true)
    expect(on).toEqual({ ...DEFAULTS, dataCollection: 'allow' })
    expect(withDataCollection(on, false)).toEqual(DEFAULTS)
    const saved: RoutingSettings = { ...DEFAULTS, fallbackCount: 1, unknownQuantPolicy: 'strict', tierWeights: { budget: 0.1, balanced: 0.2, fast: 0.3 } }
    expect(withDataCollection(saved, true)).toEqual({ ...saved, dataCollection: 'allow' })
    expect(withDataCollection(saved, true)).not.toEqual(saved)
  })

  it('leaves the settings it was given alone', () => {
    const saved: RoutingSettings = { ...DEFAULTS, tierWeights: { ...DEFAULTS.tierWeights } }
    const copy = structuredClone(saved)
    const next = withDataCollection(saved, true)
    expect(saved).toEqual(copy)
    next.tierWeights.fast = 0
    expect(saved.tierWeights.fast).toBe(1)
  })

  it('gives the problem when the stored settings can’t be read, and null otherwise', () => {
    const problem = 'The saved ranking settings couldn’t be read, so the defaults apply. Save or Restore defaults replaces them.'
    expect(switchBlocked(view({ stored: true, problem }))).toBe(problem)
    expect(switchBlocked(view())).toBeNull()
    expect(switchBlocked(null)).toBeNull()
  })
})

describe('the fixed texts', () => {
  it('are decisions.md’s', () => {
    expect(DATA_COLLECTION_LABEL).toBe('Allow providers that may store or train on prompts')
    expect(DATA_COLLECTION_HELP).toBe(
      "Off, every tier asks OpenRouter only for providers that don't collect prompt data. On, those providers can be ranked and chosen too.",
    )
    expect(NITRO_NOTE).toBe(
      'Nitro has nothing to set: Chorus adds :nitro to the model and OpenRouter sends each request to its fastest endpoint. Data collection still applies.',
    )
    expect(KEEP_HISTORY_HELP).toBe('Shorter keeps less: older samples are dropped at the next update.')
  })

  it('give Restore defaults’ title as the spec does', () => {
    expect(RESTORE_TITLE).toBe('Deletes the saved ranking settings, Data collection included: every setting goes back to its default.')
    expect(actionStates(false, false).restore).toEqual({ disabled: false, title: RESTORE_TITLE })
    expect(actionStates(true, false).restore).toEqual({ disabled: false, title: RESTORE_TITLE })
  })

  it('put Nitro’s line under Nitro, and no setting there', () => {
    const nitro = RANKING_GROUPS.find((g) => g.key === 'nitro')!
    expect(nitro.note).toBe(NITRO_NOTE)
    expect(nitro.fields).toEqual([])
    expect(RANKING_GROUPS.filter((g) => g.note).map((g) => g.key)).toEqual(['nitro'])
  })

  it('say that a save applies to the next ranking, and that a restore turns Data collection off', () => {
    expect(SAVED_NOTICE).toContain('next ranking')
    expect(SAVED_NOTICE).toContain('running sessions')
    expect(RESET_NOTICE).toContain('Data collection is off')
  })
})

describe('actionStates', () => {
  it('lets Save and Cancel through only when there is a change', () => {
    const idle = actionStates(false, false)
    expect(idle.save).toEqual({ disabled: true, title: 'Nothing has changed.' })
    expect(idle.cancel).toEqual({ disabled: true, title: 'Nothing has changed.' })
    const edited = actionStates(true, false)
    expect(edited.save.disabled).toBe(false)
    expect(edited.cancel.disabled).toBe(false)
  })

  it('locks all three while one is working, with a title that says so', () => {
    for (const dirty of [false, true]) {
      const busy = actionStates(dirty, true)
      for (const s of [busy.save, busy.cancel, busy.restore]) expect(s).toEqual({ disabled: true, title: 'Working…' })
    }
  })

  it('gives every disabled control a title', () => {
    for (const dirty of [false, true])
      for (const busy of [false, true])
        for (const s of Object.values(actionStates(dirty, busy))) expect(s.title.length > 0, `${dirty} ${busy}`).toBe(true)
  })
})

describe('saving', () => {
  const ok = (v: RoutingSettingsView) => Promise.resolve({ status: 'ok' as const, data: v })
  const refused = (error: string) => Promise.resolve({ status: 'error' as const, error })

  it('Save sends the whole object, and hands back the view Rust returns', async () => {
    const draft = { ...toDraft(DEFAULTS), fallbackCount: '1' }
    const saved = { ...DEFAULTS, dataCollection: 'allow' as const }
    const stored = view({ settings: { ...saved, fallbackCount: 1 }, stored: true })
    const send = vi.fn((_s: RoutingSettings) => ok(stored))
    expect(await saveDraft(draft, saved, send)).toEqual({ ok: true, view: stored })
    expect(send).toHaveBeenCalledTimes(1)
    const sent = send.mock.calls[0]![0]
    expect(sent).toEqual({ ...saved, fallbackCount: 1 })
    expect(Object.keys(sent).sort()).toEqual(Object.keys(DEFAULTS).sort())
  })

  it('refuses a draft that isn’t numbers without sending anything', async () => {
    const send = vi.fn((_s: RoutingSettings) => ok(view()))
    const r = await saveDraft({ ...toDraft(DEFAULTS), smoothingWindow: 'abc' }, DEFAULTS, send)
    expect(r).toEqual({ ok: false, reason: '“Speed samples averaged” needs a number.' })
    expect(send).not.toHaveBeenCalled()
  })

  it('shows Rust’s refusal with the field’s label, and leaves the draft as typed', async () => {
    const draft = { ...toDraft(DEFAULTS), fallbackCount: '6' }
    const before = { ...draft }
    const r = await saveDraft(draft, DEFAULTS, () => refused('fallbackCount must be at most 5'))
    expect(r).toEqual({ ok: false, reason: '“Fallback endpoints after the first” must be at most 5' })
    expect(draft).toEqual(before)
    expect(draft.fallbackCount).toBe('6')
  })

  it('shows a failure that isn’t a refusal as it is', async () => {
    const r = await saveDraft(toDraft(DEFAULTS), DEFAULTS, () => Promise.reject(new Error('The window closed.')))
    expect(r).toEqual({ ok: false, reason: 'Error: The window closed.' })
  })

  it('the switch sends the saved settings with only dataCollection changed', async () => {
    const saved: RoutingSettings = { ...DEFAULTS, fallbackCount: 1 }
    const stored = view({ settings: { ...saved, dataCollection: 'allow' }, stored: true })
    const send = vi.fn((_s: RoutingSettings) => ok(stored))
    expect(await saveDataCollection(saved, true, send)).toEqual({ ok: true, view: stored })
    expect(send.mock.calls[0]![0]).toEqual({ ...saved, dataCollection: 'allow' })
  })

  it('the switch’s refusal reads with labels too', async () => {
    const r = await saveDataCollection(DEFAULTS, true, () => refused('tieRatio must be at most 2'))
    expect(r).toEqual({ ok: false, reason: '“Scores this close count as a tie” must be at most 2' })
  })

  it('settle turns a view into ok and a refusal into its reason', async () => {
    expect(await settle(ok(view()))).toEqual({ ok: true, view: view() })
    expect(await settle(refused('The database is locked.'))).toEqual({ ok: false, reason: 'The database is locked.' })
  })
})

describe('the section renders, and the logic stays in the module', () => {
  it('RoutingRanking.vue takes its words and rules from routingRanking.ts', () => {
    expect(rankingVue).toMatch(/from '\.\/routingRanking'/)
    for (const name of ['DATA_COLLECTION_LABEL', 'DATA_COLLECTION_HELP', 'RANKING_GROUPS', 'defaultText', 'actionStates', 'saveDraft', 'saveDataCollection', 'switchBlocked', 'toDraft', 'isDirty'])
      expect(rankingVue).toContain(name)
    for (const name of ['fromDraft', 'refusalText', 'withDataCollection', 'defaultText', 'toDraft'])
      expect(rankingVue).not.toMatch(new RegExp(`function ${name}\\(`))
    // The texts come from the module, not a copy.
    expect(rankingVue).not.toContain('Allow providers that may')
    expect(rankingVue).not.toContain('Nitro has nothing to set')
    expect(rankingVue).not.toContain('Shorter keeps less')
    expect(rankingVue).toContain('{{ DATA_COLLECTION_LABEL }}')
    expect(rankingVue).toContain('{{ DATA_COLLECTION_HELP }}')
    expect(rankingVue).toContain('{{ g.note }}')
    expect(rankingVue).toContain('{{ f.help }}')
  })

  it('shows each field’s default beside it, and Save, Cancel and Restore defaults', () => {
    expect(rankingVue).toContain('Default: {{ defaultText(f) }}')
    expect(rankingVue).toMatch(/@click="save">Save<\/button>/)
    expect(rankingVue).toMatch(/@click="cancel">Cancel<\/button>/)
    expect(rankingVue).toMatch(/@click="restore">Restore defaults<\/button>/)
    expect(rankingVue).toContain(':title="actions.restore.title"')
  })

  it('folds the ranking settings by default, with the switch outside the fold', () => {
    const fold = rankingVue.indexOf('<details')
    expect(fold).toBeGreaterThan(-1)
    expect(rankingVue.slice(fold, rankingVue.indexOf('>', fold))).not.toMatch(/\bopen\b/)
    expect(rankingVue.indexOf('role="switch"')).toBeLessThan(fold)
    expect(rankingVue.indexOf('<h3>Data collection</h3>')).toBeLessThan(fold)
  })

  it('disables the switch with the stored-settings problem as its title, and keeps Save and Restore free of it', () => {
    expect(rankingVue).toMatch(/:disabled="!view \|\| busy \|\| !!blocked"/)
    expect(rankingVue).toContain('blocked.value ?? ')
    // Save and Restore defaults depend on the draft and on being busy, never on the problem.
    expect(rankingVue).toContain(':disabled="actions.save.disabled"')
    expect(rankingVue).toContain(':disabled="actions.restore.disabled"')
    expect(actionStates(true, false).save.disabled).toBe(false)
    expect(actionStates(false, false).restore.disabled).toBe(false)
  })

  it('uses the three settings commands, and a refused Save keeps the draft', () => {
    for (const name of ['commands.routingSettings()', 'commands.routingSettingsSave', 'commands.routingSettingsReset()']) expect(rankingVue).toContain(name)
    // The refusal branch of Save shows the reason and returns before the draft is replaced.
    expect(rankingVue).toMatch(/if \(!r\.ok\) \{\s*saveError\.value = r\.reason\s*return\s*\}\s*adopt\(r\.view\)\s*notice\.value = SAVED_NOTICE/)
  })

  it('emits saved after Save, Restore defaults and the switch', () => {
    expect(rankingVue).toMatch(/defineEmits<\{ saved: \[\] \}>/)
    expect(rankingVue.match(/emit\('saved'\)/g)).toHaveLength(3)
    expect(rankingVue).toMatch(/notice\.value = RESET_NOTICE\s*emit\('saved'\)/)
    expect(rankingVue).toMatch(/view\.value = r\.view\s*emit\('saved'\)/)
  })

  it('RoutingSettings.vue shows it after the inspector, and the inspector ranks again when it saves', () => {
    expect(settingsVue).toMatch(/import RoutingRanking from '\.\/RoutingRanking\.vue'/)
    expect(settingsVue).toContain('<RoutingRanking @saved="inspectVersion++" />')
    expect(settingsVue.indexOf('<RoutingInspect')).toBeLessThan(settingsVue.indexOf('<RoutingRanking'))
    expect(settingsVue).toContain('<RoutingInspect :version="inspectVersion" />')
  })
})
