import React from 'react'
import { CircleAlert } from 'lucide-react'
import { convertFileSrc } from '@tauri-apps/api/core'
import { ThumbGrid } from '../Gallery'
import { listPipelineInputs, revealInExplorer } from '../lib/pipeline'
import type { PipelineInputs } from '../lib/pipeline'
import { isTauri } from '../lib/tauri'

export function ResultsPage() {
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
