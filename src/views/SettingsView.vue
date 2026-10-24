<script setup lang="ts">
/** Settings: appearance, projects, providers, keys and the agent lock, and the agent CLIs found. */
import Icon from '../components/Icon.vue'
import type { IconName } from '../components/icons'
import AppearanceSettings from './settings/AppearanceSettings.vue'
import ProjectsSettings from './settings/ProjectsSettings.vue'
import AgentsSettings from './settings/AgentsSettings.vue'
import NarratorSettings from './settings/NarratorSettings.vue'
import ProvidersPanel from '../raw/ProvidersPanel.vue'
import KeysPanel from '../raw/KeysPanel.vue'
import { useViewStore, type SettingsSection } from '../stores/view'

const view = useViewStore()

const SECTIONS: { key: SettingsSection; label: string; icon: IconName }[] = [
  { key: 'appearance', label: 'Appearance', icon: 'palette' },
  { key: 'projects', label: 'Projects', icon: 'folder' },
  { key: 'providers', label: 'Providers', icon: 'plug' },
  { key: 'narrator', label: 'Narrator', icon: 'agents' },
  { key: 'keys', label: 'Keys and lock', icon: 'key' },
  { key: 'agents', label: 'Agents', icon: 'agents' },
]
</script>

<template>
  <section class="settings">
    <h1>Settings</h1>
    <div class="layout">
      <nav class="sections" aria-label="Settings sections">
        <button
          v-for="s in SECTIONS"
          :key="s.key"
          class="section"
          :class="{ on: view.settingsSection === s.key }"
          @click="view.settingsSection = s.key"
        >
          <Icon :name="s.icon" :size="16" />
          {{ s.label }}
        </button>
      </nav>
      <div class="content">
        <AppearanceSettings v-if="view.settingsSection === 'appearance'" />
        <ProjectsSettings v-else-if="view.settingsSection === 'projects'" />
        <ProvidersPanel v-else-if="view.settingsSection === 'providers'" />
        <NarratorSettings v-else-if="view.settingsSection === 'narrator'" />
        <KeysPanel v-else-if="view.settingsSection === 'keys'" />
        <AgentsSettings v-else />
      </div>
    </div>
  </section>
</template>

<style scoped>
.settings {
  padding: 28px 40px 40px;
  max-width: 1100px;
}
h1 {
  margin: 0 0 22px;
  font-size: 28px;
}
.layout {
  display: grid;
  grid-template-columns: 190px minmax(0, 1fr);
  gap: 28px;
  align-items: start;
}
.sections {
  display: flex;
  flex-direction: column;
  gap: 2px;
  position: sticky;
  top: 0;
}
.section {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-2);
  text-align: left;
  font-weight: 500;
}
.section:hover:not(:disabled) {
  background: var(--surface);
  color: var(--text);
  border-color: transparent;
}
.section.on {
  background: var(--surface-chip);
  color: var(--text);
  font-weight: 600;
}
.content {
  min-width: 0;
}
@media (max-width: 860px) {
  .settings {
    padding: 22px 20px;
  }
  .layout {
    grid-template-columns: 1fr;
  }
  .sections {
    flex-direction: row;
    flex-wrap: wrap;
    position: static;
  }
}
</style>
