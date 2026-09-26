import { call } from './tauri'

export type MediaEntry = {
  name: string
  path: string
  size_bytes: number
  modified?: string | null
}

export type PipelineInputs = {
  clips_root: string
  clips: MediaEntry[]
  audio: MediaEntry[]
  renders: MediaEntry[]
  markers: MediaEntry[]
  edits: MediaEntry[]
}

export type StoryboardShot = {
  clip: string
  in: number
  out: number
}

export type Storyboard = {
  name: string
  audio: string
  duration: number
  fps: number
  width: number
  height: number
  shots: StoryboardShot[]
  cues: TextCue[]
  beats: number[]
}

export type TextCue = {
  start: number
  end: number
  text: string
  size: number
}

export type Markers = {
  audio: string
  audio_duration_seconds?: number
  analyzed_seconds?: number
  sensitivity?: number
  min_gap_seconds?: number
  beats: number[]
  waveform: number[]
  waveform_bin_seconds?: number
}

export type PipelineEvent = {
  type: string
  run_id?: string
  plugin?: string
  message?: string
  detail?: string
  level?: string
  markers?: string
  output?: string
  metadata?: string | null
  beat_count?: number
  waveform_bins?: number
  analyzed_seconds?: number
  percent?: number
  seconds?: number
  shots?: number
  snap_cuts?: boolean
  duration_seconds?: number
  success?: boolean
  renderer?: string
  script?: string
  payload?: string
  installed_script?: string | null
  executable?: string
  artifact?: string
}

export type RendererCapability = {
  kinetic_typography: boolean
  beat_reactive_cuts: boolean
  beat_reactive_effects: boolean
  audio_mux: boolean
  expressions: boolean
  max_resolution: string | null
  watermark: boolean
}

export type RendererInfo = {
  id: string
  name: string
  execution: 'batch' | 'project_handoff'
  available: boolean
  capabilities: RendererCapability
  executable: string | null
  version: string | null
  unavailable_reason: string | null
  notes: string | null
}

export const DELIVERABLES_ROOT = 'F:\\modal-gui\\deliverables'

export const THUMBS_ROOT = 'F:\\modal-gui\\thumbs'

export const EDITS_ROOT = 'F:\\modal-gui\\edits'

export const listPipelineInputs = (clipsRoot?: string) =>
  call<PipelineInputs>('list_pipeline_inputs', { clipsRoot: clipsRoot || null })

export const readJsonFile = <T>(path: string) => call<T>('read_json_file', { path })

export const writeJsonFile = (path: string, value: unknown) =>
  call<void>('write_json_file', { path, value })

export const makeThumbnail = (path: string, time = 1.5) =>
  call<string>('make_thumbnail', { path, time })

export const revealInExplorer = (path: string) => call<void>('reveal_in_explorer', { path })

export const openWithDefault = (path: string) => call<void>('open_with_default', { path })

export const listRenderers = () =>
  call<{ renderers: RendererInfo[] }>('list_renderers').then((value) => value.renderers)

export const analyzeAudio = (request: {
  run_id: string
  audio: string
  output: string
  duration: number
  sensitivity: number
  min_gap: number
  max_beats: number
  bins: number
}) => call<void>('analyze_audio', { request })

export const renderTrailer = (request: {
  run_id: string
  mode: 'base' | 'graphics'
  audio: string
  clips_root: string
  markers?: string | null
  cues?: string | null
  output: string
  metadata?: string | null
  duration: number
  shot_seconds: number
  snap_cuts: boolean
  renderer: string
  renderer_options?: string[]
}) => call<void>('render_trailer', { request })

export const buildStoryboard = (request: {
  audio: string
  output: string
  clips_root?: string | null
  markers?: string | null
  duration?: number
  shot_seconds?: number
  snap_cuts?: boolean
}) => call<Storyboard>('build_storyboard', { request })

export const renderSpec = (request: {
  run_id: string
  spec: string
  output: string
  metadata?: string | null
  mode?: 'base' | 'graphics'
  renderer?: string
}) => call<void>('render_spec', { request })

export function formatBytes(bytes: number): string {
  if (!bytes) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  const index = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)))
  return (bytes / 1024 ** index).toFixed(index === 0 ? 0 : 1) + ' ' + units[index]
}

export function slug(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || 'run'
}
