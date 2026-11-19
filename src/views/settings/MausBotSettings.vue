<script setup lang="ts">
// P18 (18-2): Chorus's pairing with OpenMausBot. Matthew makes a pairing code in MausBot; Chorus
// redeems it and keeps the key, encrypted. With full access, Chorus starts each hand-off in its
// own worktree; without it, hand-offs start by hand.
import { computed, onMounted, ref } from 'vue'
import { commands, unwrap, type MausStatus } from '../../ipc'

const status = ref<MausStatus | null>(null)
const error = ref<string | null>(null)
const input = ref('')
const busy = ref(false)

async function run(call: () => Promise<MausStatus>): Promise<void> {
  busy.value = true
  error.value = null
  try {
    status.value = await call()
    input.value = ''
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    busy.value = false
  }
}

onMounted(() => run(() => unwrap(commands.mausbotStatus())))
const pair = () => run(() => unwrap(commands.mausbotPair(input.value)))
const forget = () => run(() => unwrap(commands.mausbotForget()))

const expires = computed(() => (status.value?.expiresAtMs ? new Date(status.value.expiresAtMs).toLocaleDateString([], { day: 'numeric', month: 'short', year: 'numeric' }) : null))
const access = computed(() => (status.value?.canChooseFolder ? 'full access' : 'chat-only access'))
</script>

<template>
  <section class="settings-section">
    <h2>MausBot</h2>
    <p class="lead">
      Chorus hands a project to OpenMausBot on a branch of its own, and watches for it to come back. Pairing lets Chorus start
      the conversation in MausBot itself; without it, Chorus gives you the message to start it by hand.
    </p>

    <p v-if="!status && !error" class="muted">Checking MausBot…</p>
    <template v-if="status">
      <p class="state">
        <span class="dot" :class="{ on: status.running }" aria-hidden="true" />
        {{ status.running ? `MausBot ${status.version ?? ''} is running` : 'MausBot isn’t running (Chorus looks for it on 127.0.0.1:8799)' }}
      </p>
      <p v-if="status.problem" class="warn">{{ status.problem }}</p>

      <template v-if="status.paired">
        <p>
          Paired · {{ access }}<template v-if="expires"> · until {{ expires }}</template>
        </p>
        <p v-if="!status.canChooseFolder" class="muted">
          With chat-only access Chorus can send the hand-off message but can’t choose its folder, so each hand-off starts by hand.
          Pair again with a full-access code to change that.
        </p>
        <p v-if="status.tokenProblem" class="warn">{{ status.tokenProblem }}</p>
        <button class="quiet" :disabled="busy" @click="forget">Forget the pairing</button>
      </template>

      <form v-else class="pair" @submit.prevent="pair">
        <label>
          <span>Pairing code or link from MausBot</span>
          <input v-model="input" type="password" autocomplete="off" spellcheck="false" placeholder="7KQ2-M9XD-H4TP or its link" :disabled="busy" />
        </label>
        <button class="primary" type="submit" :disabled="busy || !input.trim()">{{ busy ? 'Pairing…' : 'Pair' }}</button>
      </form>
      <p v-if="!status.paired" class="muted">
        In MausBot, open Settings → Phone & devices. Under Your other computers, leave Full access chosen and click Pair a
        computer, then paste its link here (Copy link) or type its code. A code works once, for a few minutes. Chorus keeps
        the key it gets, encrypted, and never shows it again. An <code>omb_sess_…</code> token works here too.
      </p>
    </template>
    <p v-if="error" class="warn" role="alert">{{ error }}</p>
  </section>
</template>

<style scoped>
.settings-section { display: grid; gap: 10px; max-width: 640px; }
h2 { margin: 0; }
.lead { margin: 0; color: var(--text-2); }
p { margin: 0; }
.state { display: flex; align-items: center; gap: 8px; }
.dot { width: 8px; height: 8px; border-radius: 50%; background: var(--text-muted); }
.dot.on { background: var(--status-ready); }
.muted { color: var(--text-muted); font-size: 12.5px; }
.warn { color: var(--status-needs); font-size: 12.5px; overflow-wrap: anywhere; }
.pair { display: flex; flex-wrap: wrap; align-items: flex-end; gap: 8px; }
.pair label { display: grid; gap: 4px; font-size: 12.5px; color: var(--text-muted); }
.pair input { width: 260px; max-width: 100%; font-family: var(--font-mono); }
code { font-family: var(--font-mono); font-size: 12px; }
</style>
