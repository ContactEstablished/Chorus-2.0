/**
 * What Settings → Routing's Data collection switch and Ranking settings show and do, as plain
 * functions: the fields with their labels and defaults, the draft the inputs edit, how a draft
 * becomes the object Rust checks, and how Rust's refusals read. Types only from the IPC layer, so
 * tests run without Tauri.
 */
import type { Result } from '../../ipc'
// These aren't re-exported by the IPC index, which no task of this phase claims.
import type { RoutingSettings, RoutingSettingsView, UnknownQuantPolicy } from '../../ipc/bindings'

export type RankingKey =
  | 'minUptimePct'
  | 'readmitUptimePct'
  | 'outageGuard5mPct'
  | 'unknownQuantPolicy'
  | 'smoothingWindow'
  | 'stableMinObservations'
  | 'observationMaxAgeDays'
  | 'snapshotMaxAgeMinutes'
  | 'tieRatio'
  | 'fallbackCount'
  | 'tierWeights.budget'
  | 'budgetMinTps'
  | 'budgetMedianFraction'
  | 'tierWeights.balanced'
  | 'tierWeights.fast'

export type RankingGroup = 'every' | 'budget' | 'balanced' | 'fast'

/** The texts decisions.md fixes. */
export const DATA_COLLECTION_LABEL = 'Allow providers that may store or train on prompts'
export const DATA_COLLECTION_HELP =
  "Off, every tier asks OpenRouter only for providers that don't collect prompt data. On, those providers can be ranked and chosen too."
export const NITRO_NOTE =
  'Nitro has nothing to set: Chorus adds :nitro to the model and OpenRouter sends each request to its fastest endpoint. Data collection still applies.'
export const KEEP_HISTORY_HELP = 'Shorter keeps less: older samples are dropped at the next update.'
export const RESTORE_TITLE = 'Deletes the saved ranking settings, Data collection included: every setting goes back to its default.'

export const SAVED_NOTICE = 'Saved. The next ranking uses these settings; running sessions and earlier selections don’t change.'
export const RESET_NOTICE = 'Restored. Every ranking setting is back to its default, and Data collection is off.'

export interface RankingField {
  /** The key path in `RoutingSettings` (`tierWeights.budget` is inside `tierWeights`). */
  key: RankingKey
  group: RankingGroup
  /** The whole label, as refusals name it. */
  label: string
  /** The label under its group's heading: without the tier's name. */
  short: string
  /** Takes a whole number (a count), not a decimal. */
  whole: boolean
  /** Shown after the input; empty when the number has none. */
  unit: string
  /** Shown under the input. */
  help: string | null
}

const field = (
  key: RankingKey,
  group: RankingGroup,
  label: string,
  short: string,
  unit = '',
  whole = false,
  help: string | null = null,
): RankingField => ({ key, group, label, short, whole, unit, help })

/** Every `RoutingSettings` field but `dataCollection`, in decisions.md's groups and order. */
export const RANKING_FIELDS: readonly RankingField[] = [
  field('minUptimePct', 'every', 'Minimum uptime over a day', 'Minimum uptime over a day', '%'),
  field('readmitUptimePct', 'every', 'Uptime to come back after a dip', 'Uptime to come back after a dip', '%'),
  field('outageGuard5mPct', 'every', 'Minimum uptime over the last 5 minutes', 'Minimum uptime over the last 5 minutes', '%'),
  field('unknownQuantPolicy', 'every', "Endpoints that don't state their precision", "Endpoints that don't state their precision"),
  field('smoothingWindow', 'every', 'Speed samples averaged', 'Speed samples averaged', '', true),
  field('stableMinObservations', 'every', 'Samples before speed history counts', 'Samples before speed history counts', '', true),
  field('observationMaxAgeDays', 'every', 'Keep speed history for', 'Keep speed history for', 'days', false, KEEP_HISTORY_HELP),
  field('snapshotMaxAgeMinutes', 'every', 'A ranking needs a Refresh after', 'A ranking needs a Refresh after', 'min'),
  field('tieRatio', 'every', 'Scores this close count as a tie', 'Scores this close count as a tie'),
  field('fallbackCount', 'every', 'Fallback endpoints after the first', 'Fallback endpoints after the first', '', true),
  field('tierWeights.budget', 'budget', 'Budget: weight on speed, the rest is price', 'Weight on speed, the rest is price'),
  field('budgetMinTps', 'budget', 'Budget: speed floor', 'Speed floor', 'tok/s'),
  field(
    'budgetMedianFraction',
    'budget',
    'Budget: or this share of the median speed, whichever is higher',
    'Or this share of the median speed, whichever is higher',
  ),
  field('tierWeights.balanced', 'balanced', 'Balanced: weight on speed', 'Weight on speed'),
  field('tierWeights.fast', 'fast', 'Fast: weight on speed', 'Weight on speed'),
]

const FIELD_BY_KEY = new Map(RANKING_FIELDS.map((f) => [f.key, f] as const))

/** The only choices of `unknownQuantPolicy`, the default first. */
export const POLICY_CHOICES: readonly { value: UnknownQuantPolicy; label: string }[] = [
  { value: 'first-party-and-verified', label: "Only the model's maker at full precision, or ones verified" },
  { value: 'strict', label: 'Never' },
]

/** `RoutingSettings::default()` as the settings show it: the bracketed defaults in decisions.md. */
const DEFAULT_TEXT: Record<RankingKey, string> = {
  minUptimePct: '99.5%',
  readmitUptimePct: '99.6%',
  outageGuard5mPct: '95%',
  unknownQuantPolicy: "Only the model's maker at full precision, or ones verified",
  smoothingWindow: '6',
  stableMinObservations: '3',
  observationMaxAgeDays: '7 days',
  snapshotMaxAgeMinutes: '60 min',
  tieRatio: '1.01',
  fallbackCount: '2',
  'tierWeights.budget': '0.3',
  budgetMinTps: '30 tok/s',
  budgetMedianFraction: '0.5',
  'tierWeights.balanced': '0.5',
  'tierWeights.fast': '1.0',
}

export function defaultText(f: RankingField): string {
  return DEFAULT_TEXT[f.key]
}

export interface RankingGroupView {
  /** `nitro` has no fields. */
  key: RankingGroup | 'nitro'
  heading: string
  fields: RankingField[]
  /** A line under the heading; Nitro's reason for having nothing to set. */
  note: string | null
}

const GROUP_HEADINGS: [RankingGroup | 'nitro', string][] = [
  ['every', 'Every tier'],
  ['budget', 'Budget'],
  ['balanced', 'Balanced'],
  ['fast', 'Fast'],
  ['nitro', 'Nitro'],
]

export const RANKING_GROUPS: readonly RankingGroupView[] = GROUP_HEADINGS.map(([key, heading]) => ({
  key,
  heading,
  fields: RANKING_FIELDS.filter((f) => f.group === key),
  note: key === 'nitro' ? NITRO_NOTE : null,
}))

/** What the inputs hold: text, so a half-typed or wrong value can be shown and refused. */
export type Draft = Record<RankingKey, string>

function read(s: RoutingSettings, key: RankingKey): number | UnknownQuantPolicy | null {
  switch (key) {
    case 'tierWeights.budget':
      return s.tierWeights.budget
    case 'tierWeights.balanced':
      return s.tierWeights.balanced
    case 'tierWeights.fast':
      return s.tierWeights.fast
    default:
      return s[key]
  }
}

export function toDraft(settings: RoutingSettings): Draft {
  const draft = {} as Draft
  for (const f of RANKING_FIELDS) draft[f.key] = String(read(settings, f.key) ?? '')
  return draft
}

/** Whether the inputs differ from the saved settings. */
export function isDirty(draft: Draft, saved: RoutingSettings): boolean {
  const base = toDraft(saved)
  return RANKING_FIELDS.some((f) => draft[f.key] !== base[f.key])
}

export type DraftResult = { ok: true; settings: RoutingSettings } | { ok: false; error: string }

const DECIMAL = /^[+-]?(\d+(\.\d*)?|\.\d+)$/
const MAX_COUNT = 0xffff_ffff // Rust takes the counts as u32

/** A number the field accepts, or the sentence that refuses it. */
function parse(text: string, f: RankingField): number | string {
  const n = DECIMAL.test(text.trim()) ? Number(text.trim()) : NaN
  if (!Number.isFinite(n)) return `“${f.label}” needs a number.`
  if (f.whole && !(Number.isInteger(n) && n >= 0 && n <= MAX_COUNT)) return `“${f.label}” needs a whole number.`
  return n
}

/**
 * The whole settings object the inputs describe, ready to send. `dataCollection` isn't a ranking
 * field: it keeps the saved value. A value that isn't a number, or isn't whole where the field needs
 * a count, is refused here with its label; Rust's `validate` does the rest.
 */
export function fromDraft(draft: Draft, saved: RoutingSettings): DraftResult {
  const num = {} as Record<RankingKey, number>
  for (const f of RANKING_FIELDS) {
    if (f.key === 'unknownQuantPolicy') continue
    const n = parse(draft[f.key], f)
    if (typeof n === 'string') return { ok: false, error: n }
    num[f.key] = n
  }
  const policy = POLICY_CHOICES.find((c) => c.value === draft.unknownQuantPolicy)
  if (!policy) return { ok: false, error: `“${FIELD_BY_KEY.get('unknownQuantPolicy')!.label}” needs one of its choices.` }
  return {
    ok: true,
    settings: {
      minUptimePct: num.minUptimePct,
      readmitUptimePct: num.readmitUptimePct,
      outageGuard5mPct: num.outageGuard5mPct,
      unknownQuantPolicy: policy.value,
      smoothingWindow: num.smoothingWindow,
      stableMinObservations: num.stableMinObservations,
      observationMaxAgeDays: num.observationMaxAgeDays,
      snapshotMaxAgeMinutes: num.snapshotMaxAgeMinutes,
      budgetMinTps: num.budgetMinTps,
      budgetMedianFraction: num.budgetMedianFraction,
      tierWeights: {
        budget: num['tierWeights.budget'],
        balanced: num['tierWeights.balanced'],
        fast: num['tierWeights.fast'],
      },
      tieRatio: num.tieRatio,
      fallbackCount: num.fallbackCount,
      dataCollection: saved.dataCollection,
    },
  }
}

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
// Longest first, so a name is never matched by a shorter one inside it.
const NAMES = RANKING_FIELDS.map((f) => f.key).sort((a, b) => b.length - a.length)
const NAME_PATTERN = new RegExp(`\\b(${NAMES.map(escape).join('|')})\\b`, 'g')

/**
 * Rust's refusal with each field's camelCase name replaced by its label in curly quotes:
 * "fallbackCount must be at most 5" reads “Fallback endpoints after the first” must be at most 5.
 * A message naming no field is returned as it is.
 */
export function refusalText(message: string): string {
  return message.replace(NAME_PATTERN, (name) => `“${FIELD_BY_KEY.get(name as RankingKey)!.label}”`)
}

/** The same settings with only `dataCollection` changed: on allows providers that may collect. */
export function withDataCollection(saved: RoutingSettings, allow: boolean): RoutingSettings {
  return { ...saved, tierWeights: { ...saved.tierWeights }, dataCollection: allow ? 'allow' : 'deny' }
}

/**
 * Why the switch can't be used: the stored settings can't be read, so flipping it would silently
 * replace them with the defaults. Save and Restore defaults stay available. Null when it can.
 */
export function switchBlocked(view: RoutingSettingsView | null): string | null {
  return view?.problem ?? null
}

export interface ControlState {
  disabled: boolean
  title: string
}

/** Save, Cancel and Restore defaults: enabled or not, and the title that says why or what they do. */
export function actionStates(dirty: boolean, busy: boolean): { save: ControlState; cancel: ControlState; restore: ControlState } {
  const off = (title: string): ControlState => ({ disabled: true, title })
  const idle = 'Nothing has changed.'
  return {
    save: busy ? off('Working…') : dirty ? { disabled: false, title: 'Saves these settings for the next ranking.' } : off(idle),
    cancel: busy ? off('Working…') : dirty ? { disabled: false, title: 'Brings back the saved values.' } : off(idle),
    restore: busy ? off('Working…') : { disabled: false, title: RESTORE_TITLE },
  }
}

/** What a Save, a Restore defaults or the switch came to. A refusal carries its reason. */
export type Outcome = { ok: true; view: RoutingSettingsView } | { ok: false; reason: string }

/** Waits for a settings command; Rust's refusal reads with labels, any other failure as it is. */
export async function settle(pending: Promise<Result<RoutingSettingsView>>): Promise<Outcome> {
  try {
    const r = await pending
    return r.status === 'ok' ? { ok: true, view: r.data } : { ok: false, reason: refusalText(r.error) }
  } catch (e) {
    return { ok: false, reason: String(e) }
  }
}

type Save = (settings: RoutingSettings) => Promise<Result<RoutingSettingsView>>

/**
 * Save: the draft becomes the whole object and goes to Rust. A draft that fails here is refused
 * without sending anything. The draft itself is never changed: a refusal leaves it as typed.
 */
export async function saveDraft(draft: Draft, saved: RoutingSettings, save: Save): Promise<Outcome> {
  const parsed = fromDraft(draft, saved)
  return parsed.ok ? settle(save(parsed.settings)) : { ok: false, reason: parsed.error }
}

/** The switch: the saved settings with only Data collection changed, saved at once. */
export function saveDataCollection(saved: RoutingSettings, allow: boolean, save: Save): Promise<Outcome> {
  return settle(save(withDataCollection(saved, allow)))
}
