import React from 'react'
import { listen } from '@tauri-apps/api/event'
import { CheckCircle2, Plus, X, Zap } from 'lucide-react'
import { EditPage } from './EditPage'
import { UsagePage } from './UsagePage'
import { GeneratePage } from './pages/GeneratePage'
import { JobsPage } from './pages/JobsPage'
import { ResultsPage } from './pages/ResultsPage'
import { SettingsPage } from './pages/SettingsPage'
import { call, isTauri } from './lib/tauri'
import type { Job, JobStage, JobKind } from './types'
import { navItems, stageLabels, type Draft, type Page, type WorkerEvent } from './app-config'

export function App() {
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
