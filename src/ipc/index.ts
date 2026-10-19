// Thin re-export of the generated bindings (bindings.ts is generated from src-tauri/src/ipc.rs;
// never edit it by hand). Import IPC from here, not from bindings.ts directly.
export { commands, events } from './bindings'
export type {
  AgentInfo,
  AgentKind,
  AgentRoute,
  AppInfo,
  BackendPulse,
  CappedKeyInfo,
  CatalogEntry,
  CatalogModel,
  CloseReport,
  CreateSession,
  CredentialSummary,
  CredentialUse,
  Detection,
  Effort,
  EffortChoice,
  Flavor,
  FleetClick,
  LaunchOptions,
  LaunchOutcome,
  LaunchRequest,
  LeakReport,
  Observed,
  Permission,
  PermissionChoice,
  Preset,
  PresetChoice,
  ProfileSave,
  ProfileSummary,
  ProviderKind,
  ProviderSave,
  ProviderSummary,
  ResumeKind,
  Role,
  SessionStatus,
  SessionSummary,
  Slot,
  VersionFit,
} from './bindings'

/** The result shape of every fallible generated command. */
export type Result<T> = { status: 'ok'; data: T } | { status: 'error'; error: string }

/** Unwraps a command result, throwing its error message so callers can show it. */
export async function unwrap<T>(pending: Promise<Result<T>>): Promise<T> {
  const res = await pending
  if (res.status === 'error') throw new Error(res.error)
  return res.data
}
