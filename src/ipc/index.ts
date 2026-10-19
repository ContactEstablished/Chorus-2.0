// Thin re-export of the generated bindings (bindings.ts is generated from src-tauri/src/ipc.rs;
// never edit it by hand). Import IPC from here, not from bindings.ts directly.
export { commands, events } from './bindings'
export type { AppInfo, BackendPulse, Flavor } from './bindings'
