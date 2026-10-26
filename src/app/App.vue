<script setup lang="ts">
/**
 * The main window's frame (PLAN P4): the rail, the title bar, the view the rail picked, the status
 * bar and the notices. Global keys, outside a terminal: Ctrl+K, Ctrl+G and zoom with Ctrl +/-/0.
 */
import { defineAsyncComponent, onMounted, onUnmounted, watch } from 'vue'
import Rail from '../shell/Rail.vue'
import TitleBar from '../shell/TitleBar.vue'
import StatusBar from '../shell/StatusBar.vue'
import Notices from '../shell/Notices.vue'
import { useAppStore } from '../stores/app'
import { useSessionsStore } from '../stores/sessions'
import { useProjectsStore } from '../stores/projects'
import { usePrefsStore } from '../stores/prefs'
import { useViewStore } from '../stores/view'
import { useNoticesStore } from '../stores/notices'
import { installDevApi } from './devApi'
import { useInboxStore } from '../stores/inbox'
import { inboxKey } from '../shell/inbox'
import { events } from '../ipc'

import OverviewView from '../views/OverviewView.vue'
import SessionsView from '../views/SessionsView.vue'
import SessionView from '../views/session/SessionView.vue'

const SettingsView = defineAsyncComponent(() => import('../views/SettingsView.vue'))
// P8's screens.
const NotesView = defineAsyncComponent(() => import('../views/NotesView.vue'))
const BotsView = defineAsyncComponent(() => import('../views/BotsView.vue'))
const DocumentsView = defineAsyncComponent(() => import('../views/DocumentsView.vue'))
const AutomationsView = defineAsyncComponent(() => import('../views/AutomationsView.vue'))
const LoopView = defineAsyncComponent(() => import('../views/LoopView.vue'))
const NewSessionDialog = defineAsyncComponent(() => import('../views/NewSessionDialog.vue'))
const CommandPalette = defineAsyncComponent(() => import('../shell/CommandPalette.vue'))

const app = useAppStore()
const sessions = useSessionsStore()
const projects = useProjectsStore()
const prefs = usePrefsStore()
const view = useViewStore()
const notices = useNoticesStore()
const inbox = useInboxStore()

function onKey(e: KeyboardEvent): void {
  const target = e.target instanceof Element ? e.target : null
  const protectedTarget = !!target?.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"]), [role="textbox"], [role="search"], .xterm')
  if (view.name === 'session' && inbox.returning?.opened === view.sessionId
      && !view.palette && !view.newSessionOpen && !document.querySelector('[aria-modal="true"], [role="alertdialog"], [role="dialog"], dialog[open]')
      && inboxKey(e, protectedTarget) === 'back') {
    view.goBack()
    e.preventDefault()
    e.stopPropagation()
    return
  }
  if (!e.ctrlKey || e.altKey || e.metaKey) return
  // A focused terminal keeps them: there they belong to its shell or agent (D49).
  if (e.target instanceof Element && e.target.closest('.xterm')) return
  const key = e.key.toLowerCase()
  const zoom = key === '=' || key === '+' ? 1 : key === '-' ? -1 : key === '0' ? 0 : null
  if (zoom !== null) {
    void prefs.zoomBy(zoom)
  } else if ((key === 'k' || key === 'g') && !e.shiftKey) {
    const mode = key === 'k' ? 'all' : 'projects'
    view.palette = view.palette === mode ? null : mode
  } else {
    return
  }
  e.preventDefault()
  e.stopPropagation()
}

// The tray and the fleet strip bring the window forward on a session, or on Needs you (P7).
let unlistenNavigate: (() => void) | null = null
async function listenNavigate(): Promise<void> {
  unlistenNavigate = await events.attentionNavigate.listen(({ payload }) => {
    view.palette = null
    if (payload.sessionId !== null) {
      view.openSession(payload.sessionId)
    } else {
      inbox.filter = 'needs'
      view.go('overview')
    }
  })
}

onMounted(async () => {
  window.addEventListener('keydown', onKey, { capture: true })
  void listenNavigate()
  await app.init()
  await Promise.all([prefs.load(), sessions.init(), projects.load()])
  if (app.info?.flavor === 'dev') installDevApi()
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKey, { capture: true })
  unlistenNavigate?.()
  sessions.dispose()
  app.dispose()
})

// The sessions store shows request failures; surface them as notices.
watch(
  () => sessions.error,
  (e) => {
    if (e) notices.error(e)
  },
)
</script>

<template>
  <div class="frame">
    <Rail @new-project="view.openSettings('projects')" @edit-project="view.openSettings('projects')" />
    <div class="main">
      <TitleBar @palette="view.palette = 'all'" />
      <main class="view scroll">
        <SettingsView v-if="view.name === 'settings'" />
        <SessionView v-else-if="view.name === 'session' && view.sessionId !== null" :session-id="view.sessionId" />
        <SessionsView v-else-if="view.name === 'sessions'" />
        <BotsView v-else-if="view.name === 'bots'" />
        <AutomationsView v-else-if="view.name === 'automations'" />
        <LoopView v-else-if="view.name === 'loop'" />
        <DocumentsView v-else-if="view.name === 'documents'" />
        <NotesView v-else-if="view.name === 'notes'" />
        <OverviewView v-else />
      </main>
      <StatusBar />
    </div>
    <NewSessionDialog v-if="view.newSessionOpen" />
    <CommandPalette v-if="view.palette" :key="view.palette" :mode="view.palette" />
    <Notices />
  </div>
</template>

<style scoped>
.frame {
  height: 100%;
  display: flex;
  min-height: 0;
}
.main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.view {
  flex: 1;
  min-height: 0;
  overflow: auto;
}
</style>
