import { call } from './tauri'

/**
 * Modal account row. `budget_limit` is the allocated credit the user declared
 * (optional) and `month_cost` is the metered cost Modal reported for the
 * current month. Neither is a balance: Modal does not expose remaining credit.
 */
export type ModalProfile = {
  id: string
  name: string
  modal_profile_name: string | null
  workspace_label: string | null
  enabled: boolean
  keychain_ref: string
  budget_limit: number | null
  reserve_amount: number
  max_concurrency: number
  priority: number
  last_used_at: string | null
  last_synced_at: string | null
  last_sync_error: string | null
  period: string
  month_cost: number
  month_intervals: number
}

export type ProfileDraft = {
  id?: string | null
  name: string
  modal_profile_name?: string | null
  workspace_label?: string | null
  keychain_ref?: string | null
  budget_limit?: number | null
  max_concurrency?: number | null
  priority?: number | null
}

export type UsageJobRow = {
  id: string
  kind: string | null
  prompt: string
  status: string
  stage: string
  profile_id: string | null
  profile_name: string | null
  created_at: string
  completed_at: string | null
  runtime_seconds: number | null
  recorded_cost: number | null
}

export type UsageObjectRow = {
  profile_id: string | null
  profile_name: string | null
  object_id: string | null
  label: string | null
  cost: number
  intervals: number
  observed_at: string | null
}

export type UsageRows = {
  period: string
  jobs: UsageJobRow[]
  objects: UsageObjectRow[]
  /** How many per-job cost records exist at all; 0 means no producer is wired up. */
  job_cost_records: number
}

export type SyncOutcome = {
  profile_id: string
  name: string
  ok: boolean
  period: string
  total: number | null
  intervals: number
  objects: number
  message: string | null
}

export const listModalProfiles = () => call<ModalProfile[]>('list_modal_profiles')

export const saveModalProfile = (profile: ProfileDraft) =>
  call<ModalProfile[]>('save_modal_profile', { profile })

export const setModalProfileEnabled = (id: string, enabled: boolean) =>
  call<ModalProfile[]>('set_modal_profile_enabled', { id, enabled })

/** Retires an account without deleting it, so past cost and job history survive. */
export const archiveModalProfile = (id: string) => call<ModalProfile[]>('archive_modal_profile', { id })

export const listUsageRows = (jobLimit = 200) => call<UsageRows>('list_usage_rows', { jobLimit })

export const syncModalBilling = (profileIds?: string[]) =>
  call<SyncOutcome[]>('sync_modal_billing', { profileIds: profileIds && profileIds.length ? profileIds : null })

/** Estimated remaining credit, only when the user declared an allocated amount. */
export function estimatedRemaining(profile: ModalProfile): number | null {
  if (profile.budget_limit === null || profile.budget_limit === undefined) return null
  return profile.budget_limit - profile.month_cost
}

export function formatUsd(value: number | null | undefined, digits = 4): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '—'
  const sign = value < 0 ? '-' : ''
  return sign + '$' + Math.abs(value).toFixed(digits)
}

/** One short line for a cell; the full sanitized text stays in the database. */
export function shortError(value: string | null | undefined, max = 96): string {
  if (!value) return ''
  const flat = value.split(/\s+/).join(' ').trim()
  return flat.length > max ? flat.slice(0, max) + '…' : flat
}

export function formatStamp(value: string | null | undefined): string {
  if (!value) return '—'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return date.toLocaleString('ko-KR', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) return '—'
  if (seconds < 60) return seconds.toFixed(0) + 's'
  const minutes = Math.floor(seconds / 60)
  return minutes + 'm ' + Math.round(seconds - minutes * 60) + 's'
}
