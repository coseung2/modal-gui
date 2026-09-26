import React from 'react'
import { convertFileSrc } from '@tauri-apps/api/core'
import { Film } from 'lucide-react'
import { formatBytes, makeThumbnail } from './lib/pipeline'
import type { MediaEntry } from './lib/pipeline'
import { isTauri } from './lib/tauri'

/** Result grid: clicking a card previews that file directly, so the user never
 * has to pick from a path dropdown. */
export function ThumbGrid({ entries, selected, onSelect }: {
  entries: MediaEntry[]
  selected: string
  onSelect: (path: string) => void
}) {
  if (entries.length === 0) {
    return <p className="dim">아직 결과물이 없습니다.</p>
  }
  return (
    <div className="thumb-grid">
      {entries.map((entry) => (
        <ThumbCard
          key={entry.path}
          entry={entry}
          active={entry.path === selected}
          onSelect={onSelect}
        />
      ))}
    </div>
  )
}

function ThumbCard({ entry, active, onSelect }: {
  entry: MediaEntry
  active: boolean
  onSelect: (path: string) => void
}) {
  const [thumb, setThumb] = React.useState('')
  const [fallback, setFallback] = React.useState(false)
  const [broken, setBroken] = React.useState(false)

  React.useEffect(() => {
    if (!isTauri) return
    let alive = true
    void makeThumbnail(entry.path)
      .then((path) => { if (alive) setThumb(path) })
      .catch(() => { if (alive) setFallback(true) })
    return () => { alive = false }
  }, [entry.path])

  return (
    <button type="button" className={'thumb-card' + (active ? ' active' : '')} onClick={() => onSelect(entry.path)}>
      <span className="thumb-frame">
        {thumb ? (
          <img src={convertFileSrc(thumb)} alt="" loading="lazy" />
        ) : fallback && isTauri && !broken ? (
          <video
            src={convertFileSrc(entry.path) + '#t=1.5'}
            preload="metadata"
            muted
            playsInline
            onError={() => setBroken(true)}
          />
        ) : (
          <Film size={18} />
        )}
      </span>
      <span className="thumb-name" title={entry.path}>{entry.name}</span>
      <span className="thumb-meta">{formatBytes(entry.size_bytes)}</span>
    </button>
  )
}
