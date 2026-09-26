import React from 'react'
import { convertFileSrc } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { ArrowDown, ArrowUp, AudioLines, CircleAlert, FolderOpen, Play, Plus, RefreshCw, Scissors, Trash2, Wand2 } from 'lucide-react'
import { ThumbGrid } from './Gallery'
import { DELIVERABLES_ROOT, EDITS_ROOT, analyzeAudio, buildStoryboard, formatBytes, listPipelineInputs, listRenderers, readJsonFile, renderSpec, revealInExplorer, slug, writeJsonFile } from './lib/pipeline'
import type { Markers, PipelineEvent, PipelineInputs, RendererInfo, Storyboard, StoryboardShot, TextCue } from './lib/pipeline'
import { isTauri } from './lib/tauri'

const DEFAULT_CUES: TextCue[] = [
  { start: 0, end: 5, text: 'PUBG: BATTLEGROUNDS', size: 86 },
  { start: 6, end: 12, text: 'UPDATE 43.1', size: 112 },
  { start: 12, end: 18, text: 'KICK. DIVE. DOMINATE.', size: 64 },
  { start: 18, end: 24, text: 'LMG BALANCE // NEW PRESSURE', size: 48 },
  { start: 24, end: 30, text: 'SURFACE + UNDERWATER COMBAT', size: 45 },
  { start: 30, end: 36, text: 'CARRY. THROW. MOVE.', size: 58 },
  { start: 36, end: 42, text: 'INSTANT ITEM USE', size: 72 },
  { start: 42, end: 48, text: 'RANKED // SEASON 43', size: 60 },
  { start: 48, end: 54, text: 'PRESETS 5  ->  10', size: 70 },
  { start: 54, end: 60, text: 'DROP IN. TAKE OVER.', size: 76 },
]

type RunState = {
  id: string
  status: 'running' | 'done' | 'failed' | 'prepared'
  percent: number
  logs: string[]
  output?: string
  execution?: 'batch' | 'project_handoff'
  artifact?: string
}

export function EditPage() {
  const [inputs, setInputs] = React.useState<PipelineInputs | null>(null)
  const [loadError, setLoadError] = React.useState('')
  const [notice, setNotice] = React.useState('')
  const [audio, setAudio] = React.useState('')
  const [clipsRoot, setClipsRoot] = React.useState('F:\\modal-gui\\h3-clips\\generated')
  const [duration, setDuration] = React.useState(60)
  const [shotSeconds, setShotSeconds] = React.useState(6)
  const [snapCuts, setSnapCuts] = React.useState(true)
  const [sensitivity, setSensitivity] = React.useState(1.1)
  const [minGap, setMinGap] = React.useState(0.3)
  const [maxBeats, setMaxBeats] = React.useState(48)
  const [markers, setMarkers] = React.useState<Markers | null>(null)
  const [markersPath, setMarkersPath] = React.useState('')
  const [spec, setSpec] = React.useState<Storyboard | null>(null)
  const [specPath, setSpecPath] = React.useState('')
  const [runs, setRuns] = React.useState<RunState[]>([])
  const [preview, setPreview] = React.useState('')
  const [playhead, setPlayhead] = React.useState(0)
  const [renderers, setRenderers] = React.useState<RendererInfo[]>([])
  const [renderer, setRenderer] = React.useState('ffmpeg')
  const markersLoaded = React.useRef(false)
  // Kept in a ref so `refresh` does not change identity every time a spec is
  // picked, which would re-bind the pipeline event listener mid-render.
  const specPathRef = React.useRef('')

  React.useEffect(() => {
    specPathRef.current = specPath
  }, [specPath])

  const active = renderers.find((item) => item.id === renderer)
  const clips = inputs?.clips ?? []
  // The pipeline cuts clips in file-name order, so the grid shows that order.
  const clipList = [...clips].sort((left, right) => left.name.localeCompare(right.name, undefined, { numeric: true }))
  const audioName = audio ? audio.split('\\').pop() || audio : ''
  const busy = runs.some((run) => run.status === 'running')
  const last = runs[0]
  const handoff = active?.execution === 'project_handoff'
  const shots = spec?.shots ?? []
  const cues = spec?.cues ?? DEFAULT_CUES
  const total = shots.reduce((sum, shot) => sum + Math.max(0, shot.out - shot.in), 0)
  const timeline: Markers | null = markers ?? (spec
    ? { audio: spec.audio, beats: spec.beats ?? [], waveform: [], analyzed_seconds: spec.duration }
    : null)

  React.useEffect(() => {
    if (!isTauri) return
    void listRenderers().then(setRenderers).catch(() => undefined)
  }, [])

  const refresh = React.useCallback(async (root?: string) => {
    if (!isTauri) {
      setLoadError('Tauri 앱에서 실행하면 F:\\modal-gui의 실제 파일을 읽어옵니다.')
      return
    }
    try {
      const value = await listPipelineInputs(root ?? clipsRoot)
      setInputs(value)
      setLoadError('')
      setAudio((current) => current || value.audio[0]?.path || '')
      const latestMarkers = value.markers?.[0]?.path
      if (latestMarkers) {
        setMarkersPath((current) => current || latestMarkers)
        if (!markersLoaded.current) {
          markersLoaded.current = true
          void readJsonFile<Markers>(latestMarkers).then(setMarkers).catch(() => undefined)
        }
      }
      const latestSpec = value.edits?.[0]?.path
      if (latestSpec) {
        setSpecPath((current) => current || latestSpec)
        if (!specPathRef.current) {
          void readJsonFile<Storyboard>(latestSpec).then(setSpec).catch(() => undefined)
        }
      }
    } catch (error) {
      setLoadError(String(error))
    }
  }, [clipsRoot])

  React.useEffect(() => {
    void refresh()
  }, [refresh])

  React.useEffect(() => {
    if (!isTauri) return
    let stop = () => {}
    void listen<PipelineEvent>('pipeline-event', (event) => {
      const message = event.payload
      if (!message.run_id) return
      setRuns((current) => current.map((run) => {
        if (run.id !== message.run_id) return run
        if (message.type === 'render_progress') {
          return { ...run, percent: message.percent ?? run.percent }
        }
        if (message.type === 'analyze_completed') {
          return { ...run, percent: 100, logs: [...run.logs, `비트 ${message.beat_count ?? 0}개 · 파형 ${message.waveform_bins ?? 0} bin`] }
        }
        if (message.type === 'render_started') {
          return { ...run, logs: [...run.logs, `샷 ${message.shots ?? 0}개 · 비트 ${message.beat_count ?? 0}개`] }
        }
        if (message.type === 'render_completed') {
          return { ...run, percent: 100, output: message.output, logs: [...run.logs, `렌더 완료 · ${message.duration_seconds ?? 0}s`] }
        }
        if (message.type === 'prepared') {
          return {
            ...run,
            status: 'prepared',
            percent: 100,
            execution: 'project_handoff',
            artifact: message.installed_script || message.script,
            logs: [...run.logs, message.message || '프로젝트를 준비했습니다.'].filter(Boolean),
          }
        }
        if (message.type === 'prepare_completed') {
          return { ...run, artifact: message.artifact ?? run.artifact }
        }
        if (message.type === 'failed') {
          return { ...run, status: 'failed', logs: [...run.logs, message.message || '실패', message.detail || ''].filter(Boolean) }
        }
        if (message.type === 'process_exit') {
          return { ...run, status: message.success ? (run.status === 'prepared' ? 'prepared' : 'done') : 'failed' }
        }
        if (message.type === 'log' && message.level === 'error') {
          return { ...run, logs: [...run.logs, message.message || ''].filter(Boolean).slice(-40) }
        }
        return run
      }))
      if (message.type === 'analyze_completed' && message.markers) {
        void readJsonFile<Markers>(message.markers).then(setMarkers).catch(() => undefined)
      }
      if (message.type === 'render_completed' && message.output) {
        setPreview(message.output)
        void refresh()
      }
    }).then((unlisten) => { stop = unlisten })
    return () => stop()
  }, [refresh])

  const startRun = (execution?: 'batch' | 'project_handoff') => {
    const id = 'run_' + Date.now() + '_' + Math.random().toString(36).slice(2, 7)
    const run: RunState = { id, status: 'running', percent: 0, logs: [], execution }
    setRuns((current) => [run, ...current].slice(0, 6))
    return id
  }

  const failRun = (id: string, error: unknown) => {
    setRuns((current) => current.map((run) => run.id === id
      ? { ...run, status: 'failed', logs: [...run.logs, String(error)] }
      : run))
  }

  const onAnalyze = async () => {
    if (!audio) {
      setNotice('음악 트랙을 선택하세요.')
      return
    }
    setNotice('')
    const output = `${DELIVERABLES_ROOT}\\beats-${slug(audio.split('\\').pop() || 'audio')}-s${sensitivity}.json`
    const id = startRun()
    setMarkersPath(output)
    try {
      await analyzeAudio({ run_id: id, audio, output, duration, sensitivity, min_gap: minGap, max_beats: maxBeats, bins: 480 })
    } catch (error) {
      failRun(id, error)
    }
  }

  const autoFill = async () => {
    if (!audio) {
      setNotice('음악 트랙을 찾지 못했습니다. 고급 설정에서 트랙을 선택하세요.')
      return null
    }
    if (clips.length === 0) {
      setNotice('클립을 찾지 못했습니다. 고급 설정에서 클립 폴더를 확인하세요.')
      return null
    }
    setNotice('')
    const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)
    const target = `${EDITS_ROOT}\\${slug(audioName.replace(/\.[^.]+$/, '') || 'storyboard')}-${stamp}.json`
    const value = await buildStoryboard({
      audio,
      output: target,
      clips_root: clipsRoot,
      markers: markersPath || null,
      duration,
      shot_seconds: shotSeconds,
      snap_cuts: snapCuts,
    })
    setSpec(value)
    setSpecPath(target)
    void refresh()
    return { spec: value, path: target }
  }

  const loadSpec = async (path: string) => {
    setSpecPath(path)
    if (!path) {
      setSpec(null)
      return
    }
    try {
      setSpec(await readJsonFile<Storyboard>(path))
      setNotice('')
    } catch (error) {
      setNotice(String(error))
    }
  }

  const onSave = async () => {
    if (!spec || !specPath) return
    try {
      await writeJsonFile(specPath, spec)
      setNotice('저장했습니다.')
    } catch (error) {
      setNotice(String(error))
    }
  }

  const onRender = async (mode: 'base' | 'graphics') => {
    if (!audio) {
      setNotice('음악 트랙을 찾지 못했습니다. 고급 설정에서 트랙을 선택하세요.')
      return
    }
    if (clips.length === 0) {
      setNotice('클립을 찾지 못했습니다. 고급 설정에서 클립 폴더를 확인하세요.')
      return
    }
    setNotice('')
    const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)
    const extension = renderer === 'autograph' ? 'mov' : 'mp4'
    const output = `${DELIVERABLES_ROOT}\\trailer-${renderer}-${mode}-${stamp}.${extension}`
    const metadata = output.replace(/\.(mp4|mov)$/, '.json')
    const id = startRun(active?.execution)
    try {
      let target = specPath
      if (!spec || !target) {
        const created = await autoFill()
        if (!created) {
          failRun(id, '스토리보드를 만들지 못했습니다.')
          return
        }
        target = created.path
      } else {
        // The CLI reads the file, so edits are written before every render.
        await writeJsonFile(target, spec)
      }
      await renderSpec({ run_id: id, spec: target, output, metadata, mode, renderer })
    } catch (error) {
      failRun(id, error)
    }
  }

  const updateShot = (index: number, patch: Partial<StoryboardShot>) => {
    setSpec((current) => current
      ? { ...current, shots: current.shots.map((shot, position) => position === index ? { ...shot, ...patch } : shot) }
      : current)
  }

  const moveShot = (index: number, delta: number) => {
    setSpec((current) => {
      if (!current) return current
      const target = index + delta
      if (target < 0 || target >= current.shots.length) return current
      const next = [...current.shots]
      const moved = next[index]
      next[index] = next[target]
      next[target] = moved
      return { ...current, shots: next }
    })
  }

  const removeShot = (index: number) => {
    setSpec((current) => current
      ? { ...current, shots: current.shots.filter((_, position) => position !== index) }
      : current)
  }

  const appendShot = () => {
    setSpec((current) => current
      ? { ...current, shots: [...current.shots, { clip: clips[0]?.path ?? '', in: 0, out: shotSeconds }] }
      : current)
  }

  const updateCue = (index: number, patch: Partial<TextCue>) => {
    setSpec((current) => current
      ? { ...current, cues: current.cues.map((cue, position) => position === index ? { ...cue, ...patch } : cue) }
      : current)
  }

  const failed = last?.status === 'failed'

  return (
    <div className="page-stack">
      <div className="toolbar">
        <div className="seg" role="group" aria-label="출력 방식">
          {renderers.length === 0 && <span className="dim">렌더러 확인 중…</span>}
          {renderers.map((item) => (
            <button
              key={item.id}
              type="button"
              className={renderer === item.id ? 'active' : ''}
              disabled={!item.available}
              title={item.available ? item.notes ?? undefined : item.unavailable_reason ?? undefined}
              aria-pressed={renderer === item.id}
              onClick={() => setRenderer(item.id)}
            >
              {item.name}
            </button>
          ))}
        </div>
        <button className="primary-action" disabled={busy} onClick={() => void onRender('graphics')}>
          <Play size={15} fill="currentColor" />
          {busy ? '진행 중…' : handoff ? '프로젝트 준비' : '렌더'}
        </button>
        {busy && last?.execution !== 'project_handoff' && <span className="run-line">{last?.percent.toFixed(0)}%</span>}
        {failed && <span className="run-line bad"><CircleAlert size={13} />실패</span>}
        <span className="dim source-line">
          {shots.length}샷 · 합계 {total.toFixed(2)}s · {audioName || '음악 없음'}
        </span>
        <button className="icon-button" onClick={() => void refresh()} title="소스 다시 읽기" aria-label="소스 다시 읽기"><RefreshCw size={15} /></button>
      </div>

      <div className="toolbar">
        <select className="spec-select" value={specPath} onChange={(event) => void loadSpec(event.target.value)} aria-label="스토리보드">
          <option value="">스토리보드 선택</option>
          {(inputs?.edits ?? []).map((item) => (
            <option key={item.path} value={item.path}>{item.name}</option>
          ))}
        </select>
        <button className="secondary-action" disabled={busy} onClick={() => void autoFill()}><Wand2 size={15} />자동 채우기</button>
        <button className="secondary-action" disabled={!spec || busy} onClick={() => void onSave()}>저장</button>
        <span className="dim source-line">{spec ? specPath.split('\\').pop() : '자동 채우기로 편집 가능한 샷 목록을 만듭니다.'}</span>
      </div>

      {(loadError || notice) && <div className="inline-warn"><CircleAlert size={14} />{loadError || notice}</div>}

      {shots.length > 0 && (
        <div className="shot-list">
          {shots.map((shot, index) => (
            <div className="shot-row" key={index}>
              <span className="shot-index">{String(index + 1).padStart(2, '0')}</span>
              <select value={shot.clip} onChange={(event) => updateShot(index, { clip: event.target.value })} aria-label={`샷 ${index + 1} 클립`}>
                {!clipList.some((clip) => clip.path === shot.clip) && <option value={shot.clip}>{shot.clip.split('\\').pop()}</option>}
                {clipList.map((clip) => (
                  <option key={clip.path} value={clip.path}>{clip.name}</option>
                ))}
              </select>
              <input className="shot-num" type="number" min={0} step={0.1} value={shot.in} onChange={(event) => updateShot(index, { in: Number(event.target.value) })} aria-label={`샷 ${index + 1} 시작`} />
              <input className="shot-num" type="number" min={0} step={0.1} value={shot.out} onChange={(event) => updateShot(index, { out: Number(event.target.value) })} aria-label={`샷 ${index + 1} 종료`} />
              <span className="shot-duration">{Math.max(0, shot.out - shot.in).toFixed(2)}s</span>
              <button className="icon-button" disabled={index === 0} onClick={() => moveShot(index, -1)} aria-label="샷 위로"><ArrowUp size={14} /></button>
              <button className="icon-button" disabled={index === shots.length - 1} onClick={() => moveShot(index, 1)} aria-label="샷 아래로"><ArrowDown size={14} /></button>
              <button className="icon-button" onClick={() => removeShot(index)} aria-label="샷 삭제"><Trash2 size={14} /></button>
            </div>
          ))}
        </div>
      )}

      {spec && (
        <div className="toolbar">
          <button className="secondary-action" onClick={appendShot}><Plus size={15} />샷 추가</button>
          <button className="secondary-action" disabled={busy} onClick={() => void onRender('base')}><Scissors size={15} />베이스 컷</button>
          <button className="secondary-action" onClick={() => void revealInExplorer(DELIVERABLES_ROOT)}><FolderOpen size={15} />산출물 폴더</button>
          {last?.artifact && (
            <button className="secondary-action" onClick={() => void revealInExplorer(last.artifact!)}><FolderOpen size={15} />준비된 스크립트</button>
          )}
        </div>
      )}

      {preview && (
        <div className="preview-wrap">
          <video
            className="preview-video"
            src={isTauri ? convertFileSrc(preview) : undefined}
            controls
            onTimeUpdate={(event) => setPlayhead(event.currentTarget.currentTime)}
          />
          <div className="preview-meta">
            <code>{preview}</code>
            <button className="link-button" onClick={() => void revealInExplorer(preview)}>탐색기에서 보기</button>
          </div>
        </div>
      )}

      <ThumbGrid entries={clipList} selected={preview} onSelect={setPreview} />

      <details className="advanced-panel">
        <summary>고급 설정<small>소재 · 비트 · 타이포 · 로그</small></summary>
        <div className="advanced-body">
          <div className="advanced-section">
            <div className="control-grid">
              <label className="field-label">음악 트랙
                <select value={audio} onChange={(event) => setAudio(event.target.value)}>
                  <option value="">선택하세요</option>
                  {(inputs?.audio ?? []).map((item) => (
                    <option key={item.path} value={item.path}>{item.name} · {formatBytes(item.size_bytes)}</option>
                  ))}
                </select>
              </label>
              <label className="field-label">비트 마커
                <select value={markersPath} onChange={(event) => {
                  const next = event.target.value
                  setMarkersPath(next)
                  if (next) void readJsonFile<Markers>(next).then(setMarkers).catch(() => undefined)
                  else setMarkers(null)
                }}>
                  <option value="">새로 분석</option>
                  {(inputs?.markers ?? []).map((item) => (
                    <option key={item.path} value={item.path}>{item.name}</option>
                  ))}
                </select>
              </label>
              <label className="field-label">클립 폴더
                <input value={clipsRoot} onChange={(event) => setClipsRoot(event.target.value)} onBlur={(event) => void refresh(event.target.value)} />
              </label>
              <label className="field-label">길이(초)
                <input type="number" min={5} max={180} value={duration} onChange={(event) => setDuration(Number(event.target.value) || 60)} />
              </label>
            </div>
          </div>

          <div className="advanced-section">
            <div className="control-grid">
              <label className="field-label">민감도 {sensitivity.toFixed(2)}
                <input type="range" min={0.5} max={2.2} step={0.05} value={sensitivity} onChange={(event) => setSensitivity(Number(event.target.value))} />
              </label>
              <label className="field-label">최소 간격 {minGap.toFixed(2)}s
                <input type="range" min={0.15} max={1.5} step={0.05} value={minGap} onChange={(event) => setMinGap(Number(event.target.value))} />
              </label>
              <label className="field-label">최대 비트
                <input type="number" min={4} max={200} value={maxBeats} onChange={(event) => setMaxBeats(Number(event.target.value) || 48)} />
              </label>
              <label className="field-label">컷 길이(초)
                <input type="number" min={1} max={20} step={0.5} value={shotSeconds} onChange={(event) => setShotSeconds(Number(event.target.value) || 6)} />
              </label>
            </div>
            <Timeline markers={timeline} duration={duration} playhead={playhead} cues={cues} shots={shots} />
            <div className="button-row">
              <button className="secondary-action" disabled={!audio || busy} onClick={() => void onAnalyze()}>
                <AudioLines size={15} />비트 분석
              </button>
              <label className="checkbox-field">
                <input type="checkbox" checked={snapCuts} onChange={(event) => setSnapCuts(event.target.checked)} />
                컷 전환을 비트에 스냅
              </label>
            </div>
          </div>

          {spec && (
            <div className="advanced-section">
              <div className="cue-list">
                {cues.map((cue, index) => (
                  <div className="cue-row" key={index}>
                    <input className="cue-text" value={cue.text} onChange={(event) => updateCue(index, { text: event.target.value })} />
                    <input className="cue-num" type="number" step={0.5} value={cue.start} onChange={(event) => updateCue(index, { start: Number(event.target.value) })} aria-label="시작" />
                    <input className="cue-num" type="number" step={0.5} value={cue.end} onChange={(event) => updateCue(index, { end: Number(event.target.value) })} aria-label="종료" />
                    <input className="cue-num" type="number" step={2} value={cue.size} onChange={(event) => updateCue(index, { size: Number(event.target.value) })} aria-label="크기" />
                    <button className="icon-button" onClick={() => setSpec((current) => current ? { ...current, cues: current.cues.filter((_, position) => position !== index) } : current)} aria-label="큐 삭제"><Trash2 size={14} /></button>
                  </div>
                ))}
              </div>
              <div className="button-row">
                <button className="secondary-action" onClick={() => setSpec((current) => {
                  if (!current) return current
                  const end = current.cues.length > 0 ? current.cues[current.cues.length - 1].end : 0
                  return { ...current, cues: [...current.cues, { start: end, end: end + 6, text: 'NEW LINE', size: 64 }] }
                })}>
                  <Plus size={15} />큐 추가
                </button>
                <button className="secondary-action" onClick={() => setSpec((current) => current ? { ...current, cues: DEFAULT_CUES } : current)}>기본값</button>
              </div>
            </div>
          )}

          {last && last.logs.length > 0 && (
            <div className="advanced-section">
              <pre className="advanced-log">{last.logs.slice(-12).join('\n')}</pre>
            </div>
          )}
        </div>
      </details>
    </div>
  )
}

function Timeline({ markers, duration, playhead, cues, shots }: {
  markers: Markers | null
  duration: number
  playhead: number
  cues: TextCue[]
  shots: StoryboardShot[]
}) {
  const waveform = markers?.waveform ?? []
  const span = markers?.analyzed_seconds || duration
  const total = shots.reduce((sum, shot) => sum + Math.max(0, shot.out - shot.in), 0) || 1
  let cursor = 0
  const blocks = shots.map((shot, index) => {
    const width = Math.max(0, shot.out - shot.in)
    const block = { index, left: cursor, width, name: shot.clip.split('\\').pop() || '' }
    cursor += width
    return block
  })
  return (
    <div className="timeline">
      <div className="shot-lane" role="img" aria-label="샷 배치">
        {blocks.map((block) => (
          <span
            className="shot-block"
            key={block.index}
            style={{ left: (block.left / total) * 100 + '%', width: (block.width / total) * 100 + '%' }}
            title={`${block.index + 1}. ${block.name} (${block.width.toFixed(2)}s)`}
          >
            {block.index + 1}
          </span>
        ))}
      </div>
      <div className="wave-lane" role="img" aria-label="오디오 파형">
        {waveform.length === 0
          ? <span className="wave-empty">비트 분석을 실행하면 파형이 표시됩니다</span>
          : waveform.map((value, index) => (
              <i key={index} style={{ height: Math.max(3, value * 100) + '%' }} />
            ))}
        {(markers?.beats ?? []).map((beat) => (
          <span className="beat-tick" key={beat} style={{ left: (beat / span) * 100 + '%' }} title={beat.toFixed(2) + 's'} />
        ))}
        <span className="playhead" style={{ left: Math.min(100, (playhead / span) * 100) + '%' }} />
      </div>
      <div className="cue-lane">
        {cues.map((cue, index) => (
          <span
            className="cue-block"
            key={index}
            style={{ left: (cue.start / span) * 100 + '%', width: Math.max(1, ((cue.end - cue.start) / span) * 100) + '%' }}
            title={`${cue.text} (${cue.start}-${cue.end}s)`}
          >
            {cue.text}
          </span>
        ))}
      </div>
      <div className="ruler">
        {Array.from({ length: Math.floor(span / 10) + 1 }, (_, index) => index * 10).map((mark) => (
          <span key={mark} style={{ left: (mark / span) * 100 + '%' }}>{mark}s</span>
        ))}
      </div>
    </div>
  )
}
