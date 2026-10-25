<script setup lang="ts">
/**
 * Settings → Phone (P15 slice 4): the Telegram bot that carries the loop's questions, stops and
 * milestones. The token goes in once and is never shown back; pairing is a code sent from the phone.
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { commands, unwrap, type RelayView } from '../../ipc'

const relay = ref<RelayView | null>(null)
const token = ref('')
const error = ref('')
const notice = ref('')
const busy = ref(false)
const now = ref(Date.now())
let timer: ReturnType<typeof setInterval> | null = null

async function load(): Promise<void> {
  try {
    relay.value = await unwrap(commands.relayStatus())
  } catch (e) {
    error.value = String(e)
  }
}

onMounted(() => {
  void load()
  timer = setInterval(() => {
    now.value = Date.now()
    void load()
  }, 3000)
})
onUnmounted(() => {
  if (timer) clearInterval(timer)
})

async function run(f: () => Promise<RelayView>, done?: string): Promise<void> {
  busy.value = true
  error.value = ''
  notice.value = ''
  try {
    relay.value = await f()
    if (done) notice.value = done
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = false
  }
}

async function saveToken(): Promise<void> {
  const t = token.value
  token.value = ''
  await run(() => unwrap(commands.relaySetToken(t)), 'Telegram accepted the token.')
}

const clearToken = () => run(() => unwrap(commands.relayClearToken()), 'Token removed.')
const toggle = () => run(() => unwrap(commands.relaySetEnabled(!relay.value?.enabled)))
const unpair = (id: string) => run(() => unwrap(commands.relayUnpair(id)), 'Unpaired.')
const pair = () => run(() => unwrap(commands.relayPairStart()))
const test = () => run(() => unwrap(commands.relayTest()), 'Test message queued.')

const STATE_TEXT: Record<string, string> = {
  off: 'Off',
  'no-token': 'No bot yet',
  connecting: 'Connecting…',
  connected: 'Connected',
  conflict: 'Another program reads this bot',
  rejected: 'Token rejected',
  offline: 'Offline',
}

const minutesLeft = computed(() => {
  const p = relay.value?.pairing
  if (!p) return 0
  return Math.max(0, Math.ceil((p.expiresAtSecs * 1000 - now.value) / 60_000))
})
</script>

<template>
  <section class="phone-settings">
    <h2>Phone</h2>
    <p>
      A Telegram bot of your own carries the loop's questions to your phone, with buttons to answer, and tells you when a run stops, reaches a
      milestone or finishes. Chorus reads the bot by long polling: nothing listens on a port.
    </p>
    <p class="muted">
      Bot chats aren't end-to-end encrypted. Messages carry question text, options and short titles, never code, diffs, files or paths, and a run
      can be set to send no question text at all.
    </p>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-if="notice" role="status">{{ notice }}</p>

    <template v-if="relay">
      <p class="state" :class="relay.state">
        <strong>{{ STATE_TEXT[relay.state] ?? relay.state }}</strong>
        <span v-if="relay.botName"> · @{{ relay.botName }}</span>
        <span v-if="relay.queued" class="muted"> · {{ relay.queued }} message{{ relay.queued === 1 ? '' : 's' }} waiting to send</span>
      </p>
      <p v-if="relay.note && relay.state !== 'connected'" class="muted">{{ relay.note }}</p>

      <h3>Bot</h3>
      <p class="muted">
        Make a bot with BotFather in Telegram (/newbot), then paste the token it gives you here. Chorus keeps it encrypted, uses it only to talk
        to Telegram, and never shows it again. A new bot means pairing again.
      </p>
      <form class="row" @submit.prevent="saveToken">
        <input
          v-model="token"
          type="password"
          autocomplete="off"
          spellcheck="false"
          :placeholder="relay.hasToken ? 'Replace the token' : 'Bot token'"
          aria-label="Bot token"
        />
        <button type="submit" class="primary" :disabled="busy || !token.trim()">{{ relay.hasToken ? 'Replace' : 'Save' }}</button>
        <button v-if="relay.hasToken" type="button" class="danger" :disabled="busy" @click="clearToken">Remove</button>
      </form>
      <label class="check">
        <input type="checkbox" :checked="relay.enabled" :disabled="busy" @change="toggle" />
        Relay on
      </label>

      <template v-if="relay.hasToken">
        <h3>Paired phones</h3>
        <ul v-if="relay.users.length" class="users">
          <li v-for="u in relay.users" :key="u.id">
            {{ u.name }}
            <button type="button" class="quiet" :disabled="busy" @click="unpair(u.id)">Unpair</button>
          </li>
        </ul>
        <p v-else class="muted">None yet.</p>
        <div v-if="relay.pairing" class="pairing">
          <p>
            In Telegram, open <strong>@{{ relay.botName }}</strong> and send:
          </p>
          <p class="code">/pair {{ relay.pairing.code }}</p>
          <p class="muted">The code works once, for {{ minutesLeft }} more minute{{ minutesLeft === 1 ? '' : 's' }}, and stops after 5 wrong tries.</p>
        </div>
        <div class="row">
          <button type="button" :disabled="busy" @click="pair">{{ relay.pairing ? 'New code' : 'Pair a phone' }}</button>
          <button type="button" :disabled="busy || !relay.users.length" @click="test">Send a test message</button>
        </div>
      </template>
    </template>
  </section>
</template>

<style scoped>
h2 {
  margin-top: 0;
}
h3 {
  margin-top: 26px;
  font-size: 15px;
}
p {
  font-size: 13px;
  line-height: 1.6;
}
.muted {
  color: var(--text-muted);
}
.error {
  color: var(--status-failed);
}
.state.connected strong {
  color: var(--status-ready);
}
.state.conflict strong,
.state.rejected strong,
.state.offline strong {
  color: var(--status-failed);
}
.row {
  display: flex;
  gap: 10px;
  align-items: center;
  margin: 10px 0;
}
.row input {
  flex: 1;
  padding: 9px 12px;
  font-family: var(--font-mono);
}
.check {
  display: inline-flex;
  gap: 8px;
  align-items: center;
  font-size: 13px;
}
.users {
  list-style: none;
  padding: 0;
  margin: 8px 0;
  display: grid;
  gap: 6px;
  font-size: 13px;
}
.users li {
  display: flex;
  gap: 10px;
  align-items: center;
}
.pairing {
  padding: 10px 14px;
  border: 1px solid var(--accent-line);
  border-radius: var(--radius-card);
  margin: 10px 0;
}
.code {
  font-family: var(--font-mono);
  font-size: 22px;
  letter-spacing: 2px;
  margin: 4px 0;
}
</style>
