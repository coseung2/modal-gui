import { invoke } from '@tauri-apps/api/core'
export const isTauri = '__TAURI_INTERNALS__' in window
export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> { return invoke<T>(command, args) }
