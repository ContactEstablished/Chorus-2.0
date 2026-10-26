/**
 * What Settings → Routing's Background observation shows and does, as plain functions: the status
 * lines, the model rows, the provider choices, and the two actions (choose a provider, stop or
 * resume a model). Types only from the IPC layer, so tests run without Tauri.
 */
import type { ProviderSummary, Result } from '../../ipc'
// These aren't re-exported by the IPC index, which no task of this phase claims.
import type { CheckOutcome, ObserverView } from '../../ipc/bindings'
import { ago, inTime } from '../newSession/routingFormat'

/** The texts decisions.md fixes. */
export const OFF_TEXT = "Off. Choose a provider to record endpoint numbers every 30 minutes with its key. It's free: no prompts or code are sent."
export const NO_CHECK_TEXT = 'No check since Chorus started.'

export const OFF_LABEL = 'Off'
export const STOP_LABEL = 'Stop observing'
export const OBSERVE_LABEL = 'Observe again'
export const NO_MODELS = 'No models yet. A model is followed once it has been refreshed.'

export interface ObserverLine {
  text: string
  /** `warn` for what needs attention: the watched provider can't be used. */
  tone: 'normal' | 'warn'
}

/** A message as a sentence: a full stop is added only when it lacks one. */
function sentence(message: string): string {
  const text = message.trim()
  if (!text) return 'no reason was given.'
  return /[.!?]$/.test(text) ? text : `${text}.`
}

/** How a check ended, to follow "Last check 12 min ago: ". */
export function outcomeText(outcome: CheckOutcome): string {
  switch (outcome.kind) {
    case 'updated':
      return outcome.count === 1 ? '1 model updated.' : `${outcome.count} models updated.`
    case 'nothing-due':
      return 'nothing was due.'
    case 'no-provider':
      return `no usable provider. ${outcome.reason}`
    case 'key-refused':
      return 'OpenRouter refused the key.'
    case 'rate-limited':
      return 'OpenRouter rate-limited the check; the next check tries again.'
    case 'failed':
      return `failed: ${sentence(outcome.message)}`
  }
}

/**
 * The status, line by line. Off, it is only the Off text: nothing is recorded or fetched, so there
 * is no check to report and none coming. On, it names the provider (and what's wrong with it), the
 * last check since Chorus started, and the next one.
 */
export function observerLines(view: ObserverView, now: number): ObserverLine[] {
  if (view.providerId === null) return [{ text: OFF_TEXT, tone: 'normal' }]
  const lines: ObserverLine[] = [{ text: `On, with ${view.providerName ?? 'a provider that no longer exists'}.`, tone: 'normal' }]
  if (view.problem) lines.push({ text: view.problem, tone: 'warn' })
  const last = view.lastCheck
  const when = last === null ? null : last.atMs === null ? 'at an unknown time' : ago(last.atMs, now)
  lines.push({ text: last === null ? NO_CHECK_TEXT : `Last check ${when}: ${outcomeText(last.outcome)}`, tone: 'normal' })
  lines.push({ text: view.nextAtMs === null ? 'Next check unknown.' : `Next check ${inTime(view.nextAtMs, now)}.`, tone: 'normal' })
  return lines
}

/** Whether the next check has come, so the status is out of date until that check records its outcome. */
export function overdue(view: ObserverView | null, now: number): boolean {
  return view !== null && view.nextAtMs !== null && now >= view.nextAtMs
}

export interface ModelRow {
  model: string
  /** "Updated 12 min ago · 40 samples", or "Not updated yet". */
  status: string
  stopped: boolean
  /** "Stopped" for a model the observer skips. */
  mark: string | null
  /** The button's text. */
  action: string
  /** What the button asks `routing_observe_model` for: a stopped model is observed again. */
  observe: boolean
}

const samples = (n: number) => `${n} ${n === 1 ? 'sample' : 'samples'}`

/** The models the observer follows, in the view's order (slug order). A stopped model keeps its data. */
export function modelRows(view: ObserverView, now: number): ModelRow[] {
  return view.models.map((m) => ({
    model: m.model,
    status: m.fetchedAtMs === null ? 'Not updated yet' : `Updated ${ago(m.fetchedAtMs, now)} · ${samples(m.sampleCount)}`,
    stopped: m.stopped,
    mark: m.stopped ? 'Stopped' : null,
    action: m.stopped ? OBSERVE_LABEL : STOP_LABEL,
    observe: m.stopped,
  }))
}

export interface ProviderChoice {
  id: number
  label: string
  disabled: boolean
  /** Why it can't be chosen; undefined when it can. */
  title: string | undefined
}

/** The OpenRouter providers. Routing fetches with the provider's own key, so one without it can't be chosen. */
export function observerProviders(providers: ProviderSummary[]): ProviderChoice[] {
  return providers
    .filter((p) => p.kind === 'openrouter')
    .map((p) =>
      p.credentialId === null
        ? {
            id: p.id,
            label: `${p.name} · API key needed`,
            disabled: true,
            title: `Routing needs ${p.name}'s own API key (a management key can't fetch endpoints).`,
          }
        : { id: p.id, label: p.name, disabled: false, title: undefined },
    )
}

/** What an action came to: the status as it now reads, or the reason it was refused. */
export type Outcome = { ok: true; view: ObserverView } | { ok: false; reason: string }

/**
 * Choosing a provider is the consent (`routing_watch`); no provider turns observation off. It then
 * reads the status again, so the view shows the provider's name and problem.
 */
export async function chooseProvider(
  providerId: number | null,
  watch: (providerId: number | null) => Promise<Result<null>>,
  load: () => Promise<Result<ObserverView>>,
): Promise<Outcome> {
  try {
    const set = await watch(providerId)
    if (set.status === 'error') return { ok: false, reason: set.error }
    const view = await load()
    return view.status === 'ok' ? { ok: true, view: view.data } : { ok: false, reason: view.error }
  } catch (e) {
    return { ok: false, reason: String(e) }
  }
}

/** Stop observing (`observe` false) or Observe again: Rust answers with the status it leaves. */
export async function setObserved(
  model: string,
  observe: boolean,
  call: (model: string, observe: boolean) => Promise<Result<ObserverView>>,
): Promise<Outcome> {
  try {
    const r = await call(model, observe)
    return r.status === 'ok' ? { ok: true, view: r.data } : { ok: false, reason: r.error }
  } catch (e) {
    return { ok: false, reason: String(e) }
  }
}
