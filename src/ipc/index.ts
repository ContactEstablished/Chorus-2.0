// Thin re-export of the generated bindings (bindings.ts is generated from src-tauri/src/ipc.rs;
// never edit it by hand). Import IPC from here, not from bindings.ts directly.
export { commands, events } from './bindings'
export type {
  AgentKind,
  AppInfo,
  BackendPulse,
  CloseReport,
  CreateSession,
  CredentialSummary,
  CredentialUse,
  FleetClick,
  Flavor,
  LeakReport,
  SessionStatus,
  SessionSummary,
} from './bindings'

/** The result shape of every fallible generated command. */
export type Result<T> = { status: 'ok'; data: T } | { status: 'error'; error: string }

/** Unwraps a command result, throwing its error message so callers can show it. */
export async function unwrap<T>(pending: Promise<Result<T>>): Promise<T> {
  const res = await pending
  if (res.status === 'error') throw new Error(res.error)
  return res.data
}
