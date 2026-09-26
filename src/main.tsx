import React from 'react'
import { createRoot } from 'react-dom/client'
import { listen } from '@tauri-apps/api/event'
import { convertFileSrc } from '@tauri-apps/api/core'
import {
  AudioLines,
  BarChart3,
  CheckCircle2,
  ChevronRight,
  CircleAlert,
  ImagePlus,
  ListTodo,
  MonitorPlay,
  Play,
  Plus,
  Scissors,
  Settings2,
  Video,
  Wand2,
  X,
  Zap,
} from 'lucide-react'
import { ThumbGrid } from './Gallery'
import { listPipelineInputs, revealInExplorer } from './lib/pipeline'
import type { PipelineInputs } from './lib/pipeline'
import './styles.css'
import { call, isTauri } from './lib/tauri'
import { EditPage } from './EditPage'
import { UsagePage } from './UsagePage'
import { listModalProfiles } from './lib/usage'
import type { ModalProfile } from './lib/usage'
import type { Job, JobKind, JobStage, JobStatus } from './types'

type Page = 'Generate' | 'Jobs' | 'Edit' | 'Results' | 'Usage' | 'Settings'
type WorkerEvent = {
  type?: string
  job_id?: string
  stage?: string
  message?: string
  code?: string
  local_output_path?: string
  function_call_id?: string
}

type Draft = {
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

const stageLabels: Record<string, string> = {
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

const modeLabels: Record<JobKind, string> = {
  t2v: 'Text → Video',
  fl2v: 'First/Last → Video',
  ref2v: 'Reference → Video',
  music: 'YuE2 Music',
}

const defaultStyle = 'English, high-energy cinematic electronic rock, punchy hybrid drums, dark synth pulses, original composition, 128 BPM'
const defaultLyrics = '[Verse]\\nRed flare rising, night turns gold\\nDrop zone calling, nerve takes hold\\n\\n[Chorus]\\nDrop in, take over\\nKick, dive, dominate\\nUpdate forty-three point one changes the game'

const navItems: Array<{ id: Page; label: string; icon: React.ComponentType<{ size?: number; strokeWidth?: number }> }> = [
  { id: 'Generate', label: '새 작업', icon: Wand2 },
  { id: 'Jobs', label: '작업 큐', icon: ListTodo },
  { id: 'Edit', label: '편집 · 모션', icon: Scissors },
  { id: 'Results', label: '결과물', icon: MonitorPlay },
  { id: 'Usage', label: '사용량', icon: BarChart3 },
  { id: 'Settings', label: '설정', icon: Settings2 },
]

function App() {
  const [page, setPage] = React.useState<Page>('Generate')
  const [jobs, setJobs] = React.useState<Job[]>([])
  const [notice, setNotice] = React.useState('')

  React.useEffect(() => {
    if (!isTauri) return
    let stop = () => {}
    void listen<WorkerEvent>('worker-event', (event) => {
      const message = event.payload
      if (!message.job_id) return
      setJobs((current) => current.map((job) => {
        if (job.id !== message.job_id) return job
        const nextLogs = message.message ? [...job.logs, message.message] : job.logs
        if (message.type === 'completed') {
          return {
            ...job,
            status: 'COMPLETED',
            stage: 'COMPLETED',
            outputPath: message.local_output_path,
            completedAt: new Date().toISOString(),
            logs: [...nextLogs, '작업이 완료되었습니다.'],
          }
        }
        if (message.type === 'failed') {
          return {
            ...job,
            status: 'FAILED',
            error: message.message || message.code || '작업이 실패했습니다.',
            logs: [...nextLogs, message.message || '작업이 실패했습니다.'],
          }
        }
        if (message.type === 'remote_attached') {
          return {
            ...job,
            status: 'RUNNING',
            functionCallId: message.function_call_id,
            logs: [...nextLogs, 'Modal 작업에 연결되었습니다.'],
          }
        }
        const stage = message.stage && stageLabels[message.stage] ? message.stage as JobStage : job.stage
        return {
          ...job,
          stage,
          status: stage === 'RESULT_DOWNLOADING' || stage === 'AUDIO_DOWNLOADING' ? 'DOWNLOADING' : 'RUNNING',
          logs: message.stage ? [...nextLogs, stageLabels[message.stage] || message.stage] : nextLogs,
        }
      }))
    }).then((unlisten) => {
      stop = unlisten
    })
    return () => stop()
  }, [])

  const submit = async (draft: Draft) => {
    const count = draft.kind === 'music' ? 1 : draft.variants
    const seedBase = draft.seed ?? Math.floor(Math.random() * 900000000)
    setNotice(count > 1 ? count + '개 작업을 큐에 넣었습니다.' : '작업을 큐에 넣었습니다.')
    for (let index = 0; index < count; index += 1) {
      const id = 'job_' + Date.now() + '_' + String(index + 1).padStart(2, '0')
      const job: Job = {
        id,
        kind: draft.kind,
        profileId: draft.profileId,
        status: 'QUEUED',
        stage: 'JOB_CREATED',
        prompt: draft.kind === 'music' ? draft.style : draft.prompt,
        inputPath: draft.inputPath,
        duration: draft.kind === 'music' ? 60 : draft.duration,
        resolution: draft.kind === 'music' ? '48 kHz stereo' : draft.width + '×' + draft.height,
        width: draft.width,
        height: draft.height,
        seed: seedBase + index,
        createdAt: new Date().toISOString(),
        logs: ['작업이 큐에 등록되었습니다.'],
      }
      setJobs((current) => [job, ...current])
      const payload = {
        id,
        prompt: draft.kind === 'music' ? draft.style : draft.prompt,
        input_path: draft.inputPath,
        duration: draft.kind === 'music' ? 60 : draft.duration,
        resolution: draft.kind === 'music' ? '48 kHz stereo' : draft.width + '×' + draft.height,
        kind: draft.kind,
        width: draft.width,
        height: draft.height,
        seed: seedBase + index,
        style: draft.style,
        lyrics: draft.lyrics,
        profile_id: draft.profileId,
      }
      if (!isTauri) continue
      try {
        await call('create_job', { job: payload })
        await call(draft.kind === 'music' ? 'start_music' : 'start_job', { job: payload })
      } catch (error) {
        const message = String(error)
        setJobs((current) => current.map((item) => item.id === id ? {
          ...item,
          status: 'FAILED',
          error: message,
          logs: [...item.logs, message],
        } : item))
      }
    }
    setPage('Jobs')
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand-lockup">
          <div className="brand-mark"><Zap size={18} fill="currentColor" /></div>
          <div>
            <strong>MODAL GUI</strong>
            <span>H3 / YuE2 STUDIO</span>
          </div>
        </div>
        <div className="workspace-chip"><span className="online-dot" /> mallagaenge / main</div>
        <nav className="primary-nav" aria-label="주 메뉴">
          {navItems.map(({ id, label, icon: Icon }) => (
            <button key={id} className={page === id ? 'nav-item active' : 'nav-item'} onClick={() => setPage(id)}>
              <Icon size={17} strokeWidth={1.8} />
              <span>{label}</span>
              {id === 'Jobs' && jobs.filter((job) => job.status === 'RUNNING' || job.status === 'QUEUED').length > 0 && (
                <em>{jobs.filter((job) => job.status === 'RUNNING' || job.status === 'QUEUED').length}</em>
              )}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="runtime-status"><span className="online-dot" /><div><b>PIPELINE READY</b><small>Modal L40S / YuE2</small></div></div>
          <div className="sidebar-version">v0.2 · local control plane</div>
        </div>
      </aside>

      <main className="workspace">
        <header className="topbar">
          <div>
            <div className="eyebrow">WORKSPACE / {page.toUpperCase()}</div>
            <h1>{page === 'Generate' ? '새 미디어 만들기' : page === 'Jobs' ? '작업 큐' : page === 'Edit' ? '편집 · 모션그래픽' : page === 'Results' ? '결과물' : page === 'Usage' ? '사용량 리포트' : '설정'}</h1>
          </div>
          <div className="topbar-actions">
            <span className="system-pill"><span className="online-dot" /> LOCAL CORE</span>
            <button className="icon-button" title="새 작업" aria-label="새 작업" onClick={() => setPage('Generate')}><Plus size={18} /></button>
          </div>
        </header>

        {notice && <div className="toast"><CheckCircle2 size={16} />{notice}<button onClick={() => setNotice('')} aria-label="알림 닫기"><X size={14} /></button></div>}

        {page === 'Generate' && <GeneratePage onSubmit={submit} />}
        {page === 'Jobs' && <JobsPage jobs={jobs} onNew={() => setPage('Generate')} />}
        {page === 'Edit' && <EditPage />}
        {page === 'Results' && <ResultsPage />}
        {page === 'Usage' && <UsagePage />}
        {page === 'Settings' && <SettingsPage />}
      </main>
    </div>
  )
}

function GeneratePage({ onSubmit }: { onSubmit: (draft: Draft) => Promise<void> }) {
  const [kind, setKind] = React.useState<JobKind>('fl2v')
  const [prompt, setPrompt] = React.useState('A cinematic supply crate ignites in a volcanic battlefield, red flare smoke, sparks and dust, dynamic tracking camera, realistic game trailer lighting, no logo, no text.')
  const [inputPath, setInputPath] = React.useState('')
  const [inputName, setInputName] = React.useState('')
  const [style, setStyle] = React.useState(defaultStyle)
  const [lyrics, setLyrics] = React.useState(defaultLyrics)
  const [duration, setDuration] = React.useState(5)
  const [resolution, setResolution] = React.useState('1344x768')
  const [variants, setVariants] = React.useState(1)
  const [seedText, setSeedText] = React.useState('')
  const [profiles, setProfiles] = React.useState<ModalProfile[]>([])
  const [profileId, setProfileId] = React.useState('')

  React.useEffect(() => {
    if (!isTauri) return
    void listModalProfiles()
      .then((value) => {
        setProfiles(value)
        setProfileId((current) => current || (value.find((item) => item.enabled)?.id ?? ''))
      })
      .catch(() => undefined)
  }, [])

  const [width, height] = resolution.split('x').map(Number)
  const isMusic = kind === 'music'
  // A stopped account is rejected by the backend, so the form only offers
  // accounts that are actually usable.
  const activeProfiles = profiles.filter((profile) => profile.enabled)
  const selectedProfile = activeProfiles.find((profile) => profile.id === profileId)
  const canSubmit = (isMusic ? style.trim().length > 0 && lyrics.trim().length > 0 : prompt.trim().length > 0 && (kind === 't2v' || inputPath.trim().length > 0)) && (!isTauri || Boolean(selectedProfile))

  const chooseFile = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0] as (File & { path?: string }) | undefined
    if (!file) return
    setInputName(file.name)
    setInputPath(file.path || file.name)
  }

  return (
    <div className="page-stack">
      <div className="mode-switch" role="tablist" aria-label="생성 모드">
        {(['t2v', 'fl2v', 'ref2v', 'music'] as JobKind[]).map((item) => (
          <button key={item} className={kind === item ? 'mode-tab active' : 'mode-tab'} onClick={() => setKind(item)} role="tab" aria-selected={kind === item}>
            {item === 'music' ? <AudioLines size={15} /> : <Video size={15} />}
            {modeLabels[item]}
          </button>
        ))}
      </div>

      <form className="studio-form" onSubmit={(event) => {
        event.preventDefault()
        void onSubmit({
          kind,
          prompt,
          inputPath,
          duration,
          width,
          height,
          variants,
          seed: seedText.trim() ? Number(seedText) : undefined,
          style,
          lyrics,
          profileId,
        })
      }}>
        {!isMusic && (
          <div className="field-row">
            <label className="field-label">참조 이미지
              <input
                value={inputPath}
                onChange={(event) => { setInputPath(event.target.value); setInputName('') }}
                placeholder="C:\assets\reference.png"
              />
            </label>
            <label className="file-button">
              <ImagePlus size={15} />{inputName || '파일 선택'}
              <input type="file" accept="image/*" onChange={chooseFile} />
            </label>
          </div>
        )}

        {isMusic
          ? <>
              <label className="field-label">스타일<textarea value={style} onChange={(event) => setStyle(event.target.value)} rows={3} /></label>
              <label className="field-label">가사<textarea value={lyrics} onChange={(event) => setLyrics(event.target.value)} rows={8} /></label>
            </>
          : <textarea className="prompt-area" value={prompt} onChange={(event) => setPrompt(event.target.value)} placeholder="장면 프롬프트" />}

        <div className="control-grid">
          {!isMusic && <label className="field-label">길이<select value={duration} onChange={(event) => setDuration(Number(event.target.value))}><option value={5}>5초</option><option value={10}>10초</option><option value={15}>15초</option></select></label>}
          {!isMusic && <label className="field-label">해상도<select value={resolution} onChange={(event) => setResolution(event.target.value)}><option value="1344x768">1344 × 768</option><option value="1152x640">1152 × 640</option><option value="896x512">896 × 512</option></select></label>}
          <label className="field-label">변주 수<select value={variants} onChange={(event) => setVariants(Number(event.target.value))}><option value={1}>1개</option><option value={2}>2개</option><option value={4}>4개</option><option value={8}>8개</option></select></label>
          <label className="field-label">Seed<input value={seedText} onChange={(event) => setSeedText(event.target.value.replace(/[^0-9]/g, ''))} placeholder="자동" inputMode="numeric" /></label>
          <label className="field-label">실행 계정
            <select value={profileId} onChange={(event) => setProfileId(event.target.value)}>
              {activeProfiles.length === 0 && <option value="">사용 중인 계정 없음</option>}
              {activeProfiles.map((profile) => <option key={profile.id} value={profile.id}>{profile.name}</option>)}
            </select>
          </label>
        </div>

        <div className="form-footer">
          <span className="target-note">
            {isTauri && !selectedProfile
              ? '사용량 탭에서 Modal 계정을 추가하거나 사용으로 전환하세요.'
              : isMusic ? 'YuE2 · 48 kHz stereo · F:\\modal-gui\\music' : 'H3 L40S · F:\\modal-gui\\h3-clips\\generated'}
          </span>
          <button className="primary-action" disabled={!canSubmit}>
            <Play size={16} fill="currentColor" />
            {isMusic ? '음악 생성' : variants > 1 ? variants + '개 생성' : '영상 생성'}
          </button>
        </div>
      </form>
    </div>
  )
}

function JobsPage({ jobs, onNew }: { jobs: Job[]; onNew: () => void }) {
  const running = jobs.filter((job) => job.status === 'RUNNING' || job.status === 'QUEUED').length
  const complete = jobs.filter((job) => job.status === 'COMPLETED').length
  const failed = jobs.filter((job) => job.status === 'FAILED').length
  return (
    <div className="page-stack">
      <div className="toolbar">
        <span className="metric-line">
          <span>진행 중 <b>{running}</b></span>
          <span>완료 <b>{complete}</b></span>
          <span>실패 <b>{failed}</b></span>
          <span>전체 <b>{jobs.length}</b></span>
        </span>
        <button className="secondary-action" onClick={onNew}><Plus size={15} />새 작업</button>
      </div>
      <section className="panel table-panel">
        {jobs.length === 0
          ? <EmptyState icon={<ListTodo size={20} />} title="아직 작업이 없습니다" detail="새 작업에서 첫 작업을 만들어보세요." />
          : <div className="job-table">{jobs.map((job) => <JobRow key={job.id} job={job} />)}</div>}
      </section>
    </div>
  )
}

function JobRow({ job }: { job: Job }) {
  const statusClass = job.status.toLowerCase()
  return <div className="job-row">
    <div className="job-kind">{job.kind === 'music' ? <AudioLines size={16} /> : <Video size={16} />}</div>
    <div className="job-copy"><strong>{job.prompt.slice(0, 76)}</strong><small>{job.id} · {job.kind ? modeLabels[job.kind] : 'H3'} · {new Date(job.createdAt).toLocaleTimeString('ko-KR', { hour: '2-digit', minute: '2-digit' })}</small></div>
    <span className={'status-pill ' + statusClass}>{job.status === 'COMPLETED' ? '완료' : job.status === 'FAILED' ? '실패' : job.status === 'QUEUED' ? '대기' : '실행 중'}</span>
    <span className="job-stage">{stageLabels[job.stage] || job.stage}</span>
    <ChevronRight size={16} className="row-arrow" />
  </div>
}

function ResultsPage() {
  const [inputs, setInputs] = React.useState<PipelineInputs | null>(null)
  const [preview, setPreview] = React.useState('')
  const [error, setError] = React.useState('')

  React.useEffect(() => {
    if (!isTauri) {
      setError('Tauri 앱에서 실행하면 F:\\modal-gui\\deliverables의 실제 파일을 읽어옵니다.')
      return
    }
    void listPipelineInputs()
      .then((value) => {
        setInputs(value)
        setPreview((current) => current || value.renders[0]?.path || '')
      })
      .catch((cause) => setError(String(cause)))
  }, [])

  return (
    <div className="page-stack">
      {error && <div className="inline-warn"><CircleAlert size={14} />{error}</div>}
      {preview && (
        <div className="preview-wrap">
          <video className="preview-video" src={isTauri ? convertFileSrc(preview) : undefined} controls />
          <div className="preview-meta">
            <code>{preview}</code>
            <button className="link-button" onClick={() => void revealInExplorer(preview)}>탐색기에서 보기</button>
          </div>
        </div>
      )}
      <ThumbGrid entries={inputs?.renders ?? []} selected={preview} onSelect={setPreview} />
    </div>
  )
}

function SettingsPage() {
  return (
    <div className="page-stack">
      <section className="panel settings-panel">
        <SettingRow label="H3 클립 소재" value="F:\\modal-gui\\h3-clips\\generated" />
        <SettingRow label="음악 출력 루트" value="F:\\modal-gui\\music" />
        <SettingRow label="산출물" value="F:\\modal-gui\\deliverables" />
        <SettingRow label="H3 런타임" value="Modal L40S · v17 dense fallback" />
        <SettingRow label="YuE2 런타임" value="Modal L40S · YuE2-3B + YuE2-Vae" />
        <SettingRow label="패치 콘텐츠" value="PUBG Update 43.1 · 2026-09-09" />
      </section>
    </div>
  )
}

function SettingRow({ label, value }: { label: string; value: string }) {
  return <div className="setting-row"><span>{label}</span><code>{value}</code></div>
}

function EmptyState({ icon, title, detail }: { icon: React.ReactNode; title: string; detail: string }) {
  return <div className="empty-state"><span>{icon}</span><strong>{title}</strong><p>{detail}</p></div>
}

createRoot(document.getElementById('root')!).render(<React.StrictMode><App /></React.StrictMode>)
