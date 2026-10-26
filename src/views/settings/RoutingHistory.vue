<script setup lang="ts">
/**
 * Settings → Routing, part 5: the inspected model's speed history (a table, one row per endpoint,
 * from the stored observations) and the Refresh log (the last 50 Refreshes, for every model).
 * Reloads when `model` or `version` changes: the parent bumps `version` after a Refresh and when
 * the ranking settings are saved, because the saved age decides which samples count.
 */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { commands } from '../../ipc'
import type { RefreshLogView, SpeedHistoryRow } from '../../ipc/bindings'
import { LOG_HELP, SPEED_HELP, logNote, logRows, settle, speedNote, speedRows } from './routingHistory'

const props = defineProps<{ model: string; version: number }>()

const rows = ref<SpeedHistoryRow[] | null>(null)
const speedError = ref<string | null>(null)
const log = ref<RefreshLogView | null>(null)
const logError = ref<string | null>(null)
const now = ref(Date.now())
const ticker = setInterval(() => (now.value = Date.now()), 15_000)
onUnmounted(() => clearInterval(ticker))

// A slow answer for an earlier model must not replace the current one.
let speedSeq = 0
async function loadSpeed(): Promise<void> {
  const mine = ++speedSeq
  const model = props.model.trim()
  if (!model) {
    rows.value = null
    speedError.value = null
    return
  }
  const r = await settle(() => commands.routingHistory(model))
  if (mine !== speedSeq) return
  rows.value = r.ok ? r.data : null
  speedError.value = r.ok ? null : r.reason
}

async function loadLog(): Promise<void> {
  const r = await settle(() => commands.routingRefreshLog())
  log.value = r.ok ? r.data : null
  logError.value = r.ok ? null : r.reason
}

onMounted(() => {
  void loadSpeed()
  void loadLog()
})
watch(
  () => props.model,
  () => void loadSpeed(),
)
watch(
  () => props.version,
  () => {
    void loadSpeed()
    void loadLog()
  },
)

const speed = computed(() => (rows.value ? speedRows(rows.value, now.value) : []))
const speedText = computed(() => speedNote(props.model, rows.value))
const entries = computed(() => (log.value ? logRows(log.value.entries, now.value) : []))
const logText = computed(() => logNote(log.value))
</script>

<template>
  <section class="history">
    <h3>History</h3>

    <h4>Speed history</h4>
    <p class="small">{{ SPEED_HELP }}</p>
    <p v-if="speedError" class="small bad" role="alert">{{ speedError }}</p>
    <p v-else-if="speedText" class="small">{{ speedText }}</p>
    <div v-else-if="speed.length" class="scroll">
      <table class="grid">
        <thead>
          <tr>
            <th>Endpoint</th>
            <th>Samples</th>
            <th>Newest sample</th>
            <th>Median speed</th>
            <th>Median latency</th>
            <th>Lowest uptime (1 day)</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="r in speed" :key="r.tag">
            <td class="mono">{{ r.tag }}</td>
            <td>{{ r.samples }}</td>
            <td>
              {{ r.newest }}
              <span v-if="r.status" class="flag">{{ r.status }}</span>
            </td>
            <td>{{ r.speed }}</td>
            <td>{{ r.latency }}</td>
            <td>{{ r.uptime }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <h4>Refresh log</h4>
    <p class="small">{{ LOG_HELP }}</p>
    <p v-if="logError" class="small bad" role="alert">The Refresh log couldn’t be loaded: {{ logError }}</p>
    <p v-else-if="logText" class="small" :class="{ warn: log?.problem }">{{ logText }}</p>
    <div v-else-if="entries.length" class="scroll">
      <table class="grid">
        <thead>
          <tr>
            <th>When</th>
            <th>Model</th>
            <th>Provider</th>
            <th>Result</th>
            <th>Endpoints tested</th>
            <th>Spent</th>
            <th>Estimate</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(e, i) in entries" :key="i">
            <td>{{ e.when }}</td>
            <td class="mono">{{ e.model }}</td>
            <td>{{ e.provider }}</td>
            <td :class="{ bad: e.failed }">{{ e.result }}</td>
            <td>{{ e.endpoints }}</td>
            <td>{{ e.spent }}</td>
            <td>{{ e.estimate }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>

<style scoped>
.history {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
h3 {
  margin: 0;
  font-size: 15px;
}
h4 {
  margin: 6px 0 0;
  font-size: 13px;
  color: var(--text-2);
}
.small {
  margin: 0;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1.5;
  max-width: 70ch;
}
.warn {
  color: var(--status-needs);
}
.bad {
  color: var(--status-failed);
}
.mono {
  font-family: var(--font-mono);
}
.scroll {
  overflow-x: auto;
}
.grid {
  border-collapse: collapse;
  font-size: 12px;
}
.grid th {
  padding: 3px 14px 3px 0;
  text-align: left;
  font-weight: 600;
  color: var(--text-muted);
}
.grid td {
  padding: 3px 14px 3px 0;
  vertical-align: top;
}
.flag {
  margin-left: 6px;
  color: var(--status-needs);
  font-weight: 600;
}
</style>
