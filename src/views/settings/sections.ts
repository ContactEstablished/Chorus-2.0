/** The Settings sections, in the order the list shows them (SettingsView.vue renders each). */
import type { IconName } from '../../components/icons'
import type { SettingsSection } from '../../stores/view'

export const SECTIONS: { key: SettingsSection; label: string; icon: IconName }[] = [
  { key: 'appearance', label: 'Appearance', icon: 'palette' },
  { key: 'projects', label: 'Projects', icon: 'folder' },
  { key: 'providers', label: 'Providers', icon: 'plug' },
  { key: 'routing', label: 'Routing', icon: 'shuffle' },
  { key: 'narrator', label: 'Narrator', icon: 'agents' },
  { key: 'phone', label: 'Phone', icon: 'bell' },
  { key: 'keys', label: 'Keys and lock', icon: 'key' },
  { key: 'agents', label: 'Agents', icon: 'agents' },
]
