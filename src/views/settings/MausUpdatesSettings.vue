<script setup lang="ts">
// D135: Chorus's messages to Matthew through OpenMausBot, in one "Chorus updates" conversation.
// Sessions send their own with the maus_send tool; while he's away, Chorus also sends one when a
// session finishes, fails or needs him, or a loop run stops. Each message is logged with what
// happened to it.
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { commands, events, unwrap, type MausUpdateRow, type MausUpdatesChange, type MausUpdatesView } from '../../ipc'

const view = ref<MausUpdatesView | null>(null)
const error = ref<string | null>(null)
const busy = ref(false)
const testResult = ref<{ ok: boolean; text: string } | null>(null)
let unlisten: (() => void) | null = null

async function load(): Promise<void> {
  try {
    view.value = await unwrap(commands.mausbotUpdatesView())
    error.value = null
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
}

onMounted(async () => {
  await load()
  unlisten = await events.mausUpdatesChanged.listen(() => void load())
})
onUnmounted(() => unlisten?.())

async function change(c: Partial<MausUpdatesChange>): Promise<void> {
  busy.value = true
  try {
    view.value = await unwrap(commands.mausbotUpdatesChange({ enabled: null, away: null, botId: null, ...c }))
    error.value = null
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = false
  }
}

async function test(): Promise<void> {
  busy.value = true
  testResult.value = null
  try {
    testResult.value = { ok: true, text: await unwrap(commands.mausbotUpdatesTest()) }
  } catch (e) {
    testResult.value = { ok: false, text: e instanceof Error ? e.message : String(e) }
  } finally {
    busy.value = false
  }
}

const botName = computed(() => view.value?.bot?.name ?? view.value?.defaultBotName ?? 'Jammie')

const kinds: Record<string, string> = { 'ad-hoc': 'From a session', done: 'Done', failed: 'Failed', 'needs-you': 'Needs you', test: 'Test' }
const when = (row: MausUpdateRow) => (row.atMs == null ? '' : new Date(row.atMs).toLocaleString([], { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' }))
const firstLines = (text: string) => text.split('\n').slice(0, 2).join(' — ')
</script>

<template>
  <section class="updates">
    <h3>Messages to you</h3>
    <p class="lead">
      When you step away, Chorus can reach you through MausBot. Claude sessions can send you a message themselves, and while you’re
      away Chorus tells you when a session finishes, fails or needs you, and when a loop run stops. Messages go to one bot, in a
      conversation called “{{ view?.conversationTitle ?? 'Chorus updates' }}”; your phone shows “{{ botName }} finished” when the bot
      echoes each one.
    </p>

    <p v-if="!view && !error" class="muted">Checking…</p>
    <template v-if="view">
      <label class="check">
        <input type="checkbox" :checked="view.enabled" :disabled="busy" @change="change({ enabled: !view.enabled })" />
        <span>Send messages to MausBot</span>
      </label>
      <label class="check">
        <input type="checkbox" :checked="view.away" :disabled="busy || !view.enabled" @change="change({ away: !view.away })" />
        <span>I’m away: message me when sessions finish, fail or need me <span class="muted">(also in the title bar and tray)</span></span>
      </label>

      <label class="field">
        <span>Bot</span>
        <select :value="view.chosenBotId ?? ''" :disabled="busy" @change="change({ botId: ($event.target as HTMLSelectElement).value })">
          <option value="">{{ view.defaultBotName }} (the default)</option>
          <option v-for="b in view.bots" :key="b.id" :value="b.id">{{ b.name }}</option>
        </select>
      </label>

      <p v-if="view.problem" class="warn">{{ view.problem }}</p>
      <p v-else class="muted">
        Messages go to {{ botName }}<template v-if="view.hasConversation">, in its “{{ view.conversationTitle }}” conversation</template
        ><template v-else>; Chorus starts the “{{ view.conversationTitle }}” conversation with the first one</template>. Each costs {{ botName }}
        one short turn. Sessions may send one every 30 seconds and ten an hour; Chorus sends at most 30 an hour in all.
      </p>

      <div class="row">
        <button class="quiet" :disabled="busy || !view.enabled" @click="test">{{ busy ? 'Sending…' : 'Send test message' }}</button>
      </div>
      <p v-if="testResult" :class="testResult.ok ? 'muted' : 'warn'" role="status">{{ testResult.text }}</p>

      <template v-if="view.recent.length">
        <h4>Recent messages</h4>
        <ul class="log">
          <li v-for="row in view.recent" :key="row.id">
            <span class="when">{{ when(row) }}</span>
            <span class="kind">{{ kinds[row.kind] ?? row.kind }}</span>
            <span class="outcome" :class="row.outcome">{{ row.outcome === 'sent' ? 'Sent' : row.outcome === 'sending' ? 'Sending' : 'Not sent' }}</span>
            <span class="text" :title="row.text">{{ firstLines(row.text) }}</span>
            <span v-if="row.reason" class="reason">{{ row.reason }}</span>
          </li>
        </ul>
      </template>
    </template>
    <p v-if="error" class="warn" role="alert">{{ error }}</p>
  </section>
</template>

<style scoped>
.updates { display: grid; gap: 10px; margin-top: 18px; padding-top: 16px; border-top: 1px solid var(--border); }
h3, h4 { margin: 0; }
h4 { font-size: 12.5px; color: var(--text-2); margin-top: 6px; }
p { margin: 0; }
.lead { color: var(--text-2); }
.muted { color: var(--text-muted); font-size: 12.5px; }
.warn { color: var(--status-needs); font-size: 12.5px; overflow-wrap: anywhere; }
.check { display: flex; align-items: flex-start; gap: 8px; }
.field { display: grid; gap: 4px; font-size: 12.5px; color: var(--text-muted); max-width: 320px; }
.row { display: flex; gap: 8px; }
.log { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; font-size: 12.5px; }
.log li { display: grid; grid-template-columns: auto auto auto 1fr; column-gap: 10px; align-items: baseline; }
.when { color: var(--text-muted); white-space: nowrap; }
.kind { color: var(--text-2); white-space: nowrap; }
.outcome { white-space: nowrap; }
.outcome.sent { color: var(--status-ready); }
.outcome.skipped { color: var(--status-needs); }
.text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
.reason { grid-column: 2 / -1; color: var(--status-needs); overflow-wrap: anywhere; }
</style>
