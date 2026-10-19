import { createApp, type Component } from 'vue'
import './styles/tokens.css'

// Each window loads the same entry; `?view=` picks what it shows. Views are imported lazily so a
// small window (the splash, the fleet strip) never loads the main window's code, which keeps the
// splash's first frame fast (spike S7).
const view = new URLSearchParams(window.location.search).get('view')

async function mount(): Promise<void> {
  if (view === 'fleet') {
    document.documentElement.classList.add('fleet')
    const { default: FleetStrip } = await import('./spikes/FleetStrip.vue')
    createApp(FleetStrip as Component).mount('#app')
  } else if (view === 'splash') {
    const { default: SplashView } = await import('./splash/SplashView.vue')
    createApp(SplashView as Component).mount('#app')
  } else {
    const [{ default: App }, { createPinia }] = await Promise.all([import('./app/App.vue'), import('pinia')])
    createApp(App as Component).use(createPinia()).mount('#app')
  }
}

void mount()
