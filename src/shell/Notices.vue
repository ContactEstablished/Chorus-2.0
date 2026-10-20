<script setup lang="ts">
/** In-app notices, bottom right, newest last. Errors stay until dismissed. */
import Icon from '../components/Icon.vue'
import { useNoticesStore } from '../stores/notices'

const notices = useNoticesStore()
</script>

<template>
  <div class="notices" aria-live="polite">
    <TransitionGroup name="rise">
      <div v-for="n in notices.list" :key="n.id" class="notice" :class="n.kind" :role="n.kind === 'error' ? 'alert' : 'status'">
        <Icon :name="n.kind === 'error' ? 'alert' : n.kind === 'success' ? 'check' : 'info'" :size="16" />
        <div class="body">
          <p class="text">{{ n.text }}</p>
          <p v-if="n.detail" class="detail">{{ n.detail }}</p>
        </div>
        <button class="dismiss" aria-label="Dismiss" title="Dismiss" @click="notices.dismiss(n.id)"><Icon name="close" :size="14" /></button>
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.notices {
  position: fixed;
  right: 16px;
  bottom: calc(var(--statusbar-h) + 14px);
  width: min(380px, calc(100vw - 32px));
  display: flex;
  flex-direction: column;
  gap: 8px;
  z-index: 50;
  pointer-events: none;
}
.notice {
  pointer-events: auto;
  display: flex;
  gap: 10px;
  align-items: flex-start;
  padding: 11px 12px;
  background: var(--surface-raised);
  border: 1px solid var(--border-strong);
  border-left: 3px solid var(--tone);
  border-radius: 10px;
  box-shadow: 0 10px 30px rgb(0 0 0 / 0.35);
  color: var(--text);
  --tone: var(--accent);
}
.notice > .icon {
  color: var(--tone);
  margin-top: 1px;
}
.notice.success {
  --tone: var(--status-ready);
}
.notice.error {
  --tone: var(--status-failed);
}
.body {
  flex: 1;
  min-width: 0;
}
.text {
  margin: 0;
  font-size: 13px;
  line-height: 1.4;
}
.detail {
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--text-2);
  overflow-wrap: anywhere;
}
.dismiss {
  padding: 2px;
  border: 0;
  background: transparent;
  color: var(--text-muted);
  display: grid;
}
.dismiss:hover:not(:disabled) {
  color: var(--text);
}
.rise-enter-active,
.rise-leave-active {
  transition:
    opacity 250ms ease,
    transform 250ms ease;
}
.rise-enter-from {
  opacity: 0;
  transform: translateY(8px);
}
.rise-leave-to {
  opacity: 0;
}
</style>
