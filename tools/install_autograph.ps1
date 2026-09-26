<#
.SYNOPSIS
  Autograph 인스톨러를 관리자 권한으로 실행합니다.

.DESCRIPTION
  Autograph은 Maxon App 카탈로그에 없고 maxon.net에서 직접 받는 별도 인스톨러입니다.
  그 인스톨러가 권한 상승을 요구하므로 사용자 권한만으로는 설치할 수 없습니다.
  이 스크립트는 인스톨러를 내려받고(없으면) UAC 승격으로 실행한 뒤, 설치된
  Autograph.exe를 찾아 보고합니다.

  설치가 끝나면 커맨드라인 렌더를 쓰기 위해 Maxon App에서 라이선스를 확인하세요.
  `mx1 license list`로 현재 할당 상태를 볼 수 있습니다.

.EXAMPLE
  pwsh -File tools\install_autograph.ps1

.EXAMPLE
  어느 폴더에서든 절대 경로로 실행할 수 있습니다.
  pwsh -File "C:\Users\coseung2\Desktop\Projects\modal-gui\tools\install_autograph.ps1"
#>

[CmdletBinding()]
param(
    [string]$InstallerPath,
    [string]$DownloadUrl = 'https://mx-app-blob-prod.maxon.net/mx-package-production/installer/windows/maxon/autograph/releases/2026.1.0/Autograph-2026.1.0-Win.exe'
)

$ErrorActionPreference = 'Stop'

# Resolve relative to the script so the working directory does not matter.
if (-not $InstallerPath) {
    $root = if ($PSScriptRoot) { $PSScriptRoot } else { (Get-Location).Path }
    $InstallerPath = Join-Path $root '..\.tooling\Autograph-2026.1.0-Win.exe'
}
$installer = [System.IO.Path]::GetFullPath($InstallerPath)

if (-not (Test-Path -LiteralPath $installer)) {
    Write-Host "인스톨러를 내려받습니다 (약 1GB): $installer"
    $parent = Split-Path -Parent $installer
    if (-not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $installer -UseBasicParsing -TimeoutSec 900
}

Write-Host "Autograph 인스톨러를 관리자 권한으로 실행합니다. UAC 창을 승인하세요."
Write-Host "설치 마법사가 뜨면 화면 안내대로 진행하세요."
try {
    $process = Start-Process -FilePath $installer -Verb RunAs -Wait -PassThru
    Write-Host "인스톨러 종료 코드: $($process.ExitCode)"
}
catch {
    Write-Warning "인스톨러를 실행하지 못했습니다: $($_.Exception.Message)"
    Write-Warning "UAC 승인이 거부되면 설치가 진행되지 않습니다."
    exit 1
}

$searchDirs = @(
    'C:\Program Files\Maxon Autograph',
    'C:\Program Files\Maxon\Autograph',
    'C:\Program Files\Maxon',
    (Join-Path $env:LOCALAPPDATA 'Programs\Maxon Autograph')
)
$autograph = $null
foreach ($dir in $searchDirs) {
    if (Test-Path -LiteralPath $dir) {
        $hit = Get-ChildItem -LiteralPath $dir -Recurse -Filter 'Autograph.exe' -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($hit) { $autograph = $hit.FullName; break }
    }
}

if ($autograph) {
    Write-Host "Autograph 탐지: $autograph"
    Write-Host ""
    Write-Host "커맨드라인 렌더 라이선스 상태를 확인하세요:"
    Write-Host "  & 'C:\Program Files\Maxon\Tools\mx1.exe' license list"
}
else {
    Write-Host "Autograph.exe를 찾지 못했습니다. 설치가 취소되었거나 다른 경로에 설치됐습니다."
}

Write-Host ""
Write-Host "확인: python tools\motion_graphics_pipeline.py plugins"
