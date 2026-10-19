import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './app/App.vue'
import FleetStrip from './spikes/FleetStrip.vue'
import './styles/tokens.css'

// Each window loads the same bundle; `?view=` picks what it shows.
const view = new URLSearchParams(window.location.search).get('view')
if (view === 'fleet') {
  document.documentElement.classList.add('fleet')
  createApp(FleetStrip).mount('#app')
} else {
  createApp(App).use(createPinia()).mount('#app')
}
