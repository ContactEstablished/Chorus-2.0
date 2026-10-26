import { describe, expect, it } from 'vitest'
import settingsView from '../SettingsView.vue?raw'
import { PATHS } from '../../components/icons'
import { SECTIONS } from './sections'

describe('the Settings sections', () => {
  it('puts Routing directly after Providers, with the shuffle icon', () => {
    const keys = SECTIONS.map((s) => s.key)
    expect(keys.indexOf('routing')).toBe(keys.indexOf('providers') + 1)
    expect(SECTIONS.find((s) => s.key === 'routing')).toEqual({ key: 'routing', label: 'Routing', icon: 'shuffle' })
  })

  it('leaves every other section as it was, in its old order', () => {
    expect(SECTIONS.filter((s) => s.key !== 'routing')).toEqual([
      { key: 'appearance', label: 'Appearance', icon: 'palette' },
      { key: 'projects', label: 'Projects', icon: 'folder' },
      { key: 'providers', label: 'Providers', icon: 'plug' },
      { key: 'narrator', label: 'Narrator', icon: 'agents' },
      { key: 'keys', label: 'Keys and lock', icon: 'key' },
      { key: 'agents', label: 'Agents', icon: 'agents' },
    ])
    expect(SECTIONS).toHaveLength(7)
  })

  it('uses icons that are drawn', () => {
    for (const s of SECTIONS) expect(PATHS).toHaveProperty(s.icon)
  })
})

describe('SettingsView.vue', () => {
  it('imports SECTIONS and no longer defines them', () => {
    expect(settingsView).toMatch(/import \{ SECTIONS \} from '\.\/settings\/sections'/)
    expect(settingsView).not.toMatch(/(const|let) SECTIONS\b/)
  })

  it('renders RoutingSettings for the routing section', () => {
    expect(settingsView).toMatch(/import RoutingSettings from '\.\/settings\/RoutingSettings\.vue'/)
    expect(settingsView).toMatch(/<RoutingSettings v-else-if="view\.settingsSection === 'routing'" \/>/)
  })

  it('renders every other section as before', () => {
    for (const [component, key] of [
      ['AppearanceSettings', 'appearance'],
      ['ProjectsSettings', 'projects'],
      ['ProvidersPanel', 'providers'],
      ['NarratorSettings', 'narrator'],
      ['KeysPanel', 'keys'],
    ] as const)
      expect(settingsView).toMatch(new RegExp(`<${component} v-(else-)?if="view\\.settingsSection === '${key}'" />`))
    // Agents is the last branch, so it takes whatever is left.
    expect(settingsView).toContain('<AgentsSettings v-else />')
  })
})
