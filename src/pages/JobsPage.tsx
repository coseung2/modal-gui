import React from 'react'
import { AudioLines, ChevronRight, ListTodo, Plus, Video } from 'lucide-react'
import type { Job } from '../types'
import { modeLabels, stageLabels } from '../app-config'

export function JobsPage({ jobs, onNew }: { jobs: Job[]; onNew: () => void }) {
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


function EmptyState({ icon, title, detail }: { icon: React.ReactNode; title: string; detail: string }) {
  return <div className="empty-state"><span>{icon}</span><strong>{title}</strong><p>{detail}</p></div>
}
