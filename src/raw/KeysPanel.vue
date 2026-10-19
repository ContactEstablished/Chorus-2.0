<script setup lang="ts">
/**
 * Stored keys and the agent-lock PIN. A key is typed once and never shown again: the list has
 * labels and fingerprints only.
 */
import { onMounted, ref } from 'vue'
import { useKeysStore } from '../stores/keys'

const keys = useKeysStore()
const label = ref('')
const secret = ref('')
const pin = ref('')
const saving = ref(false)

onMounted(() => void keys.load())

async function add(): Promise<void> {
  saving.value = true
  const ok = await keys.add(label.value, secret.value)
  // Never keep a key in the page longer than the request needs it.
  secret.value = ''
  if (ok) label.value = ''
  saving.value = false
}

async function setPin(): Promise<void> {
  saving.value = true
  await keys.setPin(pin.value)
  pin.value = ''
  saving.value = false
}

function when(secs: number | null): string {
  return secs ? new Date(secs * 1000).toLocaleString() : 'never'
}
</script>

<template>
  <section class="panel">
    <h2>Keys</h2>
    <p v-if="keys.error" class="error" role="alert">{{ keys.error }}</p>
    <table v-if="keys.keys.length" class="keys">
      <thead>
        <tr><th>Label</th><th>Fingerprint</th><th>Added</th><th>Last used</th><th /></tr>
      </thead>
      <tbody>
        <tr v-for="k in keys.keys" :key="k.id">
          <td>{{ k.label }}</td>
          <td><code>{{ k.fingerprint }}</code></td>
          <td>{{ when(k.createdAtSecs) }}</td>
          <td>{{ when(k.lastUsedAtSecs) }}</td>
          <td><button class="danger" @click="keys.remove(k.id)">Delete</button></td>
        </tr>
      </tbody>
    </table>
    <p v-else class="hint">No keys yet. Keys are encrypted for your Windows account and never shown again after saving.</p>

    <form class="row" @submit.prevent="add">
      <input v-model="label" placeholder="Label, e.g. OpenRouter" maxlength="64" autocomplete="off" />
      <input v-model="secret" type="password" placeholder="Paste the key" autocomplete="off" spellcheck="false" />
      <button type="submit" :disabled="saving || !label.trim() || !secret.trim()">Save key</button>
    </form>

    <h2>Agent lock</h2>
    <p class="hint">
      A locked session refuses Close and Restart, so a stray click can't end it. With a PIN, unlocking asks for it.
      The PIN guards against accidents, not other people: it can be cleared here without knowing it.
    </p>
    <p>PIN: <strong>{{ keys.pinSet ? 'set' : 'not set' }}</strong></p>
    <form class="row" @submit.prevent="setPin">
      <input v-model="pin" type="password" placeholder="New PIN (4 or more characters)" autocomplete="off" />
      <button type="submit" :disabled="saving || pin.length < 4">{{ keys.pinSet ? 'Change PIN' : 'Set PIN' }}</button>
      <button v-if="keys.pinSet" type="button" @click="keys.clearPin()">Clear PIN</button>
    </form>
  </section>
</template>

<style scoped>
.panel {
  max-width: 820px;
  display: grid;
  gap: 14px;
}
h2 {
  font-family: var(--font-heading);
  font-size: 20px;
  margin: 8px 0 0;
}
.keys {
  border-collapse: collapse;
  width: 100%;
}
.keys th,
.keys td {
  text-align: left;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border);
}
.keys th {
  color: var(--text-muted);
  font-weight: 500;
  font-size: 12.5px;
}
.row {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}
.row input {
  flex: 1 1 200px;
}
.hint {
  color: var(--text-muted);
  margin: 0;
}
.error {
  color: var(--status-failed);
  margin: 0;
}
code {
  font-family: var(--font-mono);
  font-size: 12.5px;
}
</style>
