import React from 'react'

export function SettingsPage() {
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
