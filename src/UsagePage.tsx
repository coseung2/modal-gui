import React from 'react'
import { CircleAlert, Pencil, Plus, RefreshCw, Trash2, X } from 'lucide-react'
import { isTauri } from './lib/tauri'
import {
  archiveModalProfile,
  estimatedRemaining,
  formatDuration,
  formatStamp,
  formatUsd,
  listModalProfiles,
  listUsageRows,
  saveModalProfile,
  setModalProfileEnabled,
  shortError,
  syncModalBilling,
} from './lib/usage'
import type { ModalProfile, ProfileDraft, SyncOutcome, UsageRows } from './lib/usage'

type Form = {
  id: string | null
  name: string
  modal_profile_name: string
  workspace_label: string
  keychain_ref: string
  budget_limit: string
  max_concurrency: number
  priority: number
}

type Segment = 'jobs' | 'objects'

const blankForm = (): Form => ({
  id: null,
  name: '',
  modal_profile_name: '',
  workspace_label: '',
  keychain_ref: '',
  budget_limit: '',
  max_concurrency: 1,
  priority: 0,
})

const formOf = (profile: ModalProfile): Form => ({
  id: profile.id,
  name: profile.name,
  modal_profile_name: profile.modal_profile_name ?? '',
  workspace_label: profile.workspace_label ?? '',
  keychain_ref: profile.keychain_ref ?? '',
  budget_limit: profile.budget_limit === null ? '' : String(profile.budget_limit),
  max_concurrency: profile.max_concurrency,
  priority: profile.priority,
})

const statusLabel: Record<string, string> = {
  QUEUED: '대기',
  ASSIGNING: '할당 중',
  RUNNING: '실행 중',
  DOWNLOADING: '다운로드',
  COMPLETED: '완료',
  FAILED: '실패',
  CANCELLED: '취소',
}

const kindLabel: Record<string, string> = { t2v: 'T2V', fl2v: 'FL2V', ref2v: 'Ref2V', music: 'YuE2' }

export function UsagePage() {
  const [profiles, setProfiles] = React.useState<ModalProfile[]>([])
  const [rows, setRows] = React.useState<UsageRows | null>(null)
  const [form, setForm] = React.useState<Form | null>(null)
  const [view, setView] = React.useState<Segment>('jobs')
  const [pendingDelete, setPendingDelete] = React.useState('')
  const [busy, setBusy] = React.useState('')
  const [error, setError] = React.useState('')
  const [notice, setNotice] = React.useState('')

  const reload = React.useCallback(async () => {
    if (!isTauri) {
      setError('Tauri 앱에서 실행하면 Modal 계정과 실제 사용 기록을 불러옵니다.')
      return
    }
    try {
      const [nextProfiles, nextRows] = await Promise.all([listModalProfiles(), listUsageRows()])
      setProfiles(nextProfiles)
      setRows(nextRows)
      setError('')
    } catch (cause) {
      setError(String(cause))
    }
  }, [])

  React.useEffect(() => {
    void reload()
  }, [reload])

  const runSync = async (ids?: string[]) => {
    setBusy('sync')
    setNotice('')
    try {
      const outcomes: SyncOutcome[] = await syncModalBilling(ids)
      const failed = outcomes.filter((item) => !item.ok)
      setNotice(
        failed.length === 0
          ? outcomes.length + '개 계정을 Modal billing report로 동기화했습니다.'
          : '동기화 완료 ' + (outcomes.length - failed.length) + '개 · 실패 ' + failed.length + '개 — ' +
            failed.map((item) => item.name + ': ' + (item.message || '알 수 없는 오류')).join(' / '),
      )
      await reload()
    } catch (cause) {
      setError(String(cause))
    } finally {
      setBusy('')
    }
  }

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    if (!form) return
    const draft: ProfileDraft = {
      id: form.id,
      name: form.name.trim(),
      modal_profile_name: form.modal_profile_name.trim() || null,
      workspace_label: form.workspace_label.trim() || null,
      keychain_ref: form.keychain_ref.trim() || null,
      budget_limit: form.budget_limit.trim() === '' ? null : Number(form.budget_limit),
      max_concurrency: form.max_concurrency,
      priority: form.priority,
    }
    if (!draft.name) {
      setError('계정 이름을 입력하세요.')
      return
    }
    setBusy('save')
    try {
      setProfiles(await saveModalProfile(draft))
      setForm(null)
      setError('')
      setNotice('계정을 저장했습니다.')
    } catch (cause) {
      setError(String(cause))
    } finally {
      setBusy('')
    }
  }

  const toggle = async (profile: ModalProfile) => {
    setBusy(profile.id)
    try {
      setProfiles(await setModalProfileEnabled(profile.id, !profile.enabled))
      setError('')
    } catch (cause) {
      setError(String(cause))
    } finally {
      setBusy('')
    }
  }

  const archive = async (profile: ModalProfile) => {
    setBusy(profile.id)
    try {
      setProfiles(await archiveModalProfile(profile.id))
      setPendingDelete('')
      setNotice(profile.name + ' 계정을 보관 처리했습니다. 실행 기록과 비용 이력은 그대로 남고, 목록에서만 빠집니다.')
      await reload()
    } catch (cause) {
      setError(String(cause))
    } finally {
      setBusy('')
    }
  }

  const enabledCount = profiles.filter((profile) => profile.enabled).length
  const totalCost = profiles.reduce((sum, profile) => sum + profile.month_cost, 0)
  const period = rows?.period ?? profiles[0]?.period ?? '—'

  return (
    <div className="page-stack">
      {error && <div className="inline-warn"><CircleAlert size={14} />{error}</div>}

      <section className="panel usage-section">
        <div className="toolbar">
          <span className="metric-line">
            <span>계정 <b>{profiles.length}</b></span>
            <span>사용 중 <b>{enabledCount}</b></span>
            <span>이번달 계량 합계 <b>{formatUsd(totalCost)}</b></span>
            <span>기간 <b>{period}</b></span>
          </span>
          <button className="secondary-action" onClick={() => void runSync()} disabled={busy !== '' || profiles.length === 0}>
            <RefreshCw size={14} />{busy === 'sync' ? '동기화 중…' : '전체 동기화'}
          </button>
          <button className="secondary-action" onClick={() => { setForm(form ? null : blankForm()); setError('') }}>
            <Plus size={15} />계정 추가
          </button>
        </div>
        {notice && <div className="usage-notice">{notice}<button onClick={() => setNotice('')} aria-label="알림 닫기"><X size={12} /></button></div>}

        {form && (
          <form className="acct-form" onSubmit={submit}>
            <label className="field-label">계정 이름
              <input value={form.name} onChange={(event) => setForm({ ...form, name: event.target.value })} placeholder="예: H3 전용 계정" />
            </label>
            <label className="field-label">Modal 프로필
              <input value={form.modal_profile_name} onChange={(event) => setForm({ ...form, modal_profile_name: event.target.value })} placeholder="mallagaenge" />
            </label>
            <label className="field-label">워크스페이스
              <input value={form.workspace_label} onChange={(event) => setForm({ ...form, workspace_label: event.target.value })} placeholder="선택" />
            </label>
            <label className="field-label">할당 크레딧 ($)
              <input value={form.budget_limit} onChange={(event) => setForm({ ...form, budget_limit: event.target.value.replace(/[^0-9.]/g, '') })} placeholder="미설정" inputMode="decimal" />
            </label>
            <label className="field-label">자격증명 참조
              <input value={form.keychain_ref} onChange={(event) => setForm({ ...form, keychain_ref: event.target.value })} placeholder="keychain:modal/<프로필>" />
            </label>
            <div className="acct-form-actions">
              <button className="primary-action" type="submit" disabled={busy === 'save'}>{form.id ? '수정 저장' : '계정 추가'}</button>
              <button className="secondary-action" type="button" onClick={() => setForm(null)}>취소</button>
            </div>
            <p className="acct-form-note">토큰 값은 저장하지 않습니다. 참조 문자열만 기록하고 실제 자격증명은 로컬 Modal 프로필/키체인에 그대로 둡니다.</p>
          </form>
        )}

        <div className="table-scroll">
          <table className="dense-table accounts-table">
            <thead>
              <tr>
                <th>계정</th>
                <th>Modal 프로필</th>
                <th>상태</th>
                <th className="num">이번달 사용액</th>
                <th className="num">크레딧</th>
                <th>마지막 동기화</th>
                <th className="actions">액션</th>
              </tr>
            </thead>
            <tbody>
              {profiles.length === 0 && (
                <tr><td colSpan={7} className="table-empty">등록된 Modal 계정이 없습니다. 계정 추가로 시작하세요.</td></tr>
              )}
              {profiles.map((profile) => (
                <tr key={profile.id} className={profile.enabled ? undefined : 'off'}>
                  <td>
                    <strong>{profile.name}</strong>
                    <small>{profile.workspace_label || '워크스페이스 미지정'}{profile.keychain_ref ? ' · ' + profile.keychain_ref : ''}</small>
                  </td>
                  <td><code>{profile.modal_profile_name || '미연결'}</code></td>
                  <td><span className={profile.enabled ? 'state-chip on' : 'state-chip off'}>{profile.enabled ? '사용' : '중지'}</span></td>
                  <td className="num">
                    <strong>{formatUsd(profile.month_cost)}</strong>
                    <small>{profile.month_intervals}개 구간</small>
                  </td>
                  <td className="num"><CreditCell profile={profile} /></td>
                  <td>
                    <span>{formatStamp(profile.last_synced_at)}</span>
                    {profile.last_sync_error && <small className="bad">{shortError(profile.last_sync_error)}</small>}
                  </td>
                  <td className="actions">
                    {pendingDelete === profile.id ? (
                      <>
                        <button className="row-action danger" onClick={() => void archive(profile)} disabled={busy !== ''}>보관 확인</button>
                        <button className="row-action" onClick={() => setPendingDelete('')}>취소</button>
                      </>
                    ) : (
                      <>
                        <button className="row-action" onClick={() => void runSync([profile.id])} disabled={busy !== ''} title="이 계정 동기화"><RefreshCw size={13} /></button>
                        <button className="row-action" onClick={() => void toggle(profile)} disabled={busy !== ''} title={profile.enabled ? '중지' : '사용'}>{profile.enabled ? '중지' : '사용'}</button>
                        <button className="row-action" onClick={() => { setForm(formOf(profile)); setError('') }} title="편집"><Pencil size={13} /></button>
                        <button className="row-action" onClick={() => setPendingDelete(profile.id)} title="보관 (기록 유지)"><Trash2 size={13} /></button>
                      </>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div className="usage-note">
          <CircleAlert size={15} />
          <span>
            Modal CLI/SDK는 잔여 크레딧을 제공하지 않습니다. 위 금액은 workspace billing report의 계량 비용이고,
            추정 잔여는 직접 입력한 할당 크레딧이 있을 때만 표시합니다. 각 계정은 자기 로컬 Modal 프로필로 실행되며 전역 프로필은 바뀌지 않습니다.
          </span>
        </div>
      </section>

      <section className="panel usage-section">
        <div className="toolbar">
          <span className="metric-line">
            <span>기간 <b>{period}</b></span>
            <span>작업 <b>{rows?.jobs.length ?? 0}</b></span>
            <span>계량 객체 <b>{rows?.objects.length ?? 0}</b></span>
            {rows && rows.job_cost_records === 0 && <span className="muted">호출별 비용 기록 0건</span>}
          </span>
          <div className="seg">
            <button className={view === 'jobs' ? 'active' : ''} onClick={() => setView('jobs')}>작업별</button>
            <button className={view === 'objects' ? 'active' : ''} onClick={() => setView('objects')}>앱·리소스별</button>
          </div>
        </div>
        <div className="table-scroll">
          {view === 'jobs' ? <JobsTable rows={rows} /> : <ObjectsTable rows={rows} />}
        </div>
        <div className="usage-note">
          <CircleAlert size={15} />
          <span>
            Modal은 비용을 작업이 아니라 앱/구간 단위로 보고합니다. 설치된 Modal 1.5.2에는 호출별 billed cost 필드가 없어
            작업별 사용액을 만들 수 있는 producer가 아직 없습니다. 기록이 들어오면 그대로 표시되고, 지금은 —입니다.
          </span>
        </div>
      </section>
    </div>
  )
}

function CreditCell({ profile }: { profile: ModalProfile }) {
  const remaining = estimatedRemaining(profile)
  if (remaining === null) {
    return <><span className="muted">미설정</span><small>할당 크레딧 미입력</small></>
  }
  return (
    <>
      <strong>{formatUsd(remaining, 2)}</strong>
      <small>추정 잔여 · 할당 {formatUsd(profile.budget_limit, 2)} − 사용 {formatUsd(profile.month_cost, 2)}</small>
    </>
  )
}

function promptSummary(prompt: string) {
  const flat = prompt.replace(/\s+/g, ' ').trim()
  if (!flat) return '(빈 프롬프트)'
  return flat.length > 72 ? flat.slice(0, 72) + '…' : flat
}

function JobsTable({ rows }: { rows: UsageRows | null }) {
  const jobs = rows?.jobs ?? []
  if (jobs.length === 0) {
    return <p className="table-empty">기록된 작업이 없습니다. 새 작업을 실행하면 계정과 함께 여기에 쌓입니다.</p>
  }
  const noCostProducer = (rows?.job_cost_records ?? 0) === 0
  return (
    <table className="dense-table">
      <thead>
        <tr>
          <th>작업</th>
          <th>사용 계정</th>
          <th>완료시각</th>
          <th className="num" title={noCostProducer ? '호출별 비용을 기록하는 producer가 아직 없습니다.' : undefined}>사용액</th>
          <th>상태</th>
        </tr>
      </thead>
      <tbody>
        {jobs.map((job) => (
          <tr key={job.id}>
            <td>
              <strong>{promptSummary(job.prompt)}</strong>
              <small>{job.kind ? kindLabel[job.kind] ?? job.kind : '영상'} · {job.id}</small>
            </td>
            <td>{job.profile_name || job.profile_id || '—'}</td>
            <td>
              <span>{formatStamp(job.completed_at || job.created_at)}</span>
              <small>{job.completed_at ? '소요 ' + formatDuration(job.runtime_seconds) : statusLabel[job.status] ?? job.status}</small>
            </td>
            <td className="num">
              {job.recorded_cost === null
                ? <span className="muted">—</span>
                : <strong>{formatUsd(job.recorded_cost)}</strong>}
            </td>
            <td><span className={'status-pill ' + job.status.toLowerCase()}>{statusLabel[job.status] ?? job.status}</span></td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}

function ObjectsTable({ rows }: { rows: UsageRows | null }) {
  const objects = rows?.objects ?? []
  if (objects.length === 0) {
    return <p className="table-empty">동기화된 계량 기록이 없습니다. 계정을 동기화하면 앱·리소스별 비용이 표시됩니다.</p>
  }
  return (
    <table className="dense-table">
      <thead>
        <tr>
          <th>계정</th>
          <th>앱 · 리소스</th>
          <th>Object</th>
          <th className="num">구간</th>
          <th className="num">금액</th>
        </tr>
      </thead>
      <tbody>
        {objects.map((object) => (
          <tr key={object.profile_id + '|' + object.object_id + '|' + object.label}>
            <td>{object.profile_name || object.profile_id || '—'}</td>
            <td><strong>{object.label || '이름 없음'}</strong></td>
            <td><code>{object.object_id || '—'}</code></td>
            <td className="num">{object.intervals}</td>
            <td className="num"><strong>{formatUsd(object.cost)}</strong></td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}
