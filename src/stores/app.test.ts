import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { AppInfo, BackendPulse } from '../ipc'

const ipc = vi.hoisted(() => ({
  appInfo: vi.fn<() => Promise<AppInfo>>(),
  pulseHandler: null as ((event: { payload: BackendPulse }) => void) | null,
  unlisten: vi.fn(),
}))

vi.mock('../ipc', () => ({
  commands: { appInfo: ipc.appInfo },
  events: {
    backendPulse: {
      listen: async (handler: (event: { payload: BackendPulse }) => void) => {
        ipc.pulseHandler = handler
        return ipc.unlisten
      },
    },
  },
}))

const { useAppStore } = await import('./app')

const INFO: AppInfo = {
  productName: 'Chorus',
  version: '0.1.0',
  identifier: 'com.contactestablished.chorus2.dev',
  flavor: 'dev',
  dataDir: 'D:\\data\\com.contactestablished.chorus2.dev',
  homeDir: null,
  tauriVersion: '2.12.4',
}

describe('app store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    ipc.appInfo.mockReset()
    ipc.unlisten.mockReset()
    ipc.pulseHandler = null
  })

  it('loads app info and follows the back-end pulse', async () => {
    ipc.appInfo.mockResolvedValue(INFO)
    const store = useAppStore()
    await store.init()

    expect(store.info).toEqual(INFO)
    expect(store.pulse).toBeNull()

    ipc.pulseHandler?.({ payload: { seq: 3, uptimeSecs: 3 } })
    expect(store.pulse).toEqual({ seq: 3, uptimeSecs: 3 })

    store.dispose()
    expect(ipc.unlisten).toHaveBeenCalledOnce()
  })

  it('shows the failure instead of hiding it', async () => {
    ipc.appInfo.mockRejectedValue(new Error('IPC down'))
    const store = useAppStore()
    await store.init()

    expect(store.info).toBeNull()
    expect(store.error).toBe('IPC down')
  })
})
