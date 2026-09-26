import React from 'react'
import { BarChart3, ListTodo, MonitorPlay, Scissors, Settings2, Wand2 } from 'lucide-react'
import type { JobKind } from './types'

export type Page = 'Generate' | 'Jobs' | 'Edit' | 'Results' | 'Usage' | 'Settings'
export type WorkerEvent = {
  type?: string
  job_id?: string
  stage?: string
  message?: string
  code?: string
  local_output_path?: string
  function_call_id?: string
}

export type Draft = {
  kind: JobKind
  prompt: string
  inputPath: string
  duration: number
  width: number
  height: number
  variants: number
  seed?: number
  style: string
  lyrics: string
  profileId: string
}

export const stageLabels: Record<string, string> = {
  JOB_CREATED: '작업 생성',
  PROFILE_ASSIGNED: '프로필 할당',
  INPUT_UPLOADING: '입력 업로드',
  CONTAINER_STARTING: '컨테이너 시작',
  GPU_READY: 'GPU 준비',
  MODEL_LOADING: '모델 로딩',
  MODEL_READY: '모델 준비 완료',
  PREPROCESSING: '입력 전처리',
  GENERATING: '영상 생성',
  MUSIC_GENERATING: '음악 생성',
  AUDIO_DOWNLOADING: '오디오 저장',
  ENCODING: '인코딩',
  SAVING: '결과 저장',
  RESULT_DOWNLOADING: '결과 다운로드',
  COMPLETED: '완료',
}

export const modeLabels: Record<JobKind, string> = {
  t2v: 'Text → Video',
  fl2v: 'First/Last → Video',
  ref2v: 'Reference → Video',
  music: 'YuE2 Music',
}

export const defaultStyle = 'English, high-energy cinematic electronic rock, punchy hybrid drums, dark synth pulses, original composition, 128 BPM'
export const defaultLyrics = '[Verse]\\nRed flare rising, night turns gold\\nDrop zone calling, nerve takes hold\\n\\n[Chorus]\\nDrop in, take over\\nKick, dive, dominate\\nUpdate forty-three point one changes the game'

export const navItems: Array<{ id: Page; label: string; icon: React.ComponentType<{ size?: number; strokeWidth?: number }> }> = [
  { id: 'Generate', label: '새 작업', icon: Wand2 },
  { id: 'Jobs', label: '작업 큐', icon: ListTodo },
  { id: 'Edit', label: '편집 · 모션', icon: Scissors },
  { id: 'Results', label: '결과물', icon: MonitorPlay },
  { id: 'Usage', label: '사용량', icon: BarChart3 },
  { id: 'Settings', label: '설정', icon: Settings2 },
]
