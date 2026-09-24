# Modal GUI

로컬에서 이미지와 프롬프트를 입력하면 **Modal GPU에서 MiniMax H3 오픈소스 모델을 직접 실행**하고, 생성 과정과 결과 영상을 한 화면에서 관리하는 Tauri 데스크톱 애플리케이션입니다.

> 이 프로젝트에서 "MiniMax API"는 사용하지 않습니다. H3 모델은 Modal 컨테이너 내부에서 로컬 추론하며, 데스크톱 앱은 작업 생성·계정/워크스페이스 선택·상태 추적·결과 수신을 담당합니다.

---

## 1. 목표

사용자는 Modal 웹 UI를 직접 탐색하지 않고 아래 흐름만 수행하면 됩니다.

1. 입력 이미지 선택
2. 영상 프롬프트 입력
3. 생성 옵션 설정
4. `영상 생성` 클릭
5. 앱에서 작업 단계와 로그 확인
6. 완료된 영상을 앱에서 재생하거나 로컬 폴더에서 열기

앱은 다음 정보를 한 화면에서 제공하는 것을 목표로 합니다.

- 등록된 Modal 프로필/워크스페이스 목록
- 프로필별 활성/비활성 상태
- 프로필별 로컬 할당 예산과 사용량
- 현재 실행 중인 작업 수
- 작업 큐
- 각 작업의 현재 단계
- 실시간 로그
- 실패 원인 및 재시도 상태
- 결과 영상/썸네일/메타데이터
- 사용했던 입력 이미지와 프롬프트

---

## 2. 확정 기술 스택

### Desktop

- Tauri 2
- React
- TypeScript
- Vite
- Tailwind CSS
- shadcn/ui
- Lucide Icons
- Zustand

### Native / Application Core

- Rust
- Tokio
- SQLite
- SQLx

### Modal Controller

- Python
- Modal Python SDK
- PyInstaller sidecar

### Video / Media

- HTML5 `<video>`
- FFmpeg / ffprobe sidecar

### Secret Storage

- OS Keychain / Credential Manager

---

## 3. 전체 아키텍처

```text
┌───────────────────────────────────────────────┐
│              Tauri Desktop App               │
│                                               │
│  React / TypeScript                           │
│  ├─ Generate                                  │
│  ├─ Jobs                                      │
│  ├─ Modal Pool                                │
│  ├─ Results                                   │
│  └─ Settings                                  │
└──────────────────────┬────────────────────────┘
                       │ Tauri invoke / events
                       ▼
┌───────────────────────────────────────────────┐
│                 Rust Core                     │
│                                               │
│  ├─ Job Manager                               │
│  ├─ Queue / Scheduler                         │
│  ├─ Modal Profile Pool                        │
│  ├─ SQLite                                    │
│  ├─ Result Manager                            │
│  ├─ Notification Manager                      │
│  └─ Python Sidecar Manager                    │
└──────────────────────┬────────────────────────┘
                       │ JSON Lines
                       ▼
┌───────────────────────────────────────────────┐
│             Python Modal Sidecar              │
│                                               │
│  ├─ Modal SDK                                 │
│  ├─ Profile credential injection              │
│  ├─ Function spawn / reconnect                │
│  ├─ Log streaming                             │
│  ├─ Input upload                              │
│  └─ Result download                           │
└──────────────────────┬────────────────────────┘
                       │
                       ▼
                  Modal Workspace
                       │
                       ▼
┌───────────────────────────────────────────────┐
│               H3 GPU Worker                   │
│                                               │
│  ├─ MiniMax H3 open-source model              │
│  ├─ CUDA / PyTorch                            │
│  ├─ Image preprocessing                       │
│  ├─ Image → Video inference                   │
│  ├─ Encode result                             │
│  └─ Save output                               │
└───────────────────────────────────────────────┘
```

---

## 4. 컴포넌트 책임 분리

### React

React는 화면 표시와 사용자 입력만 담당합니다.

React에서 직접 다음 작업을 수행하지 않습니다.

- Modal SDK 호출
- Modal credential 접근
- SQLite 직접 접근
- Python 프로세스 직접 관리
- 파일 시스템 핵심 로직
- 계정/워크스페이스 스케줄링

### Rust Core

애플리케이션의 Source of Truth입니다.

담당:

- Job 생성 및 상태 관리
- SQLite 영속화
- 작업 큐
- Modal 프로필 선택
- Python sidecar lifecycle
- JSONL 메시지 검증
- Tauri event 발행
- 결과 파일 관리
- 앱 재시작 후 작업 복구
- OS 알림

### Python Sidecar

Modal SDK 어댑터입니다.

담당:

- 특정 Modal 프로필 credential 적용
- Modal Function 실행
- FunctionCall ID 반환
- 로그 스트림 수신
- Modal 작업 상태 조회
- 실행 중 FunctionCall 재연결
- 입력 파일 전달
- 결과 파일 다운로드

Python sidecar에는 UI 상태나 장기 영속 상태를 두지 않습니다.

### H3 Worker

영상 생성만 담당하는 stateless inference worker를 기본 원칙으로 합니다.

모델 캐시와 Modal Volume은 사용할 수 있지만, 앱의 Job 상태를 Worker 내부 상태에 의존하지 않습니다.

---

## 5. Job 상태 모델

### 상위 상태

```text
QUEUED
ASSIGNING
RUNNING
DOWNLOADING
COMPLETED
FAILED
CANCELLED
```

### 실행 단계

```text
JOB_CREATED
PROFILE_ASSIGNED
INPUT_UPLOADING
CONTAINER_STARTING
GPU_READY
MODEL_LOADING
MODEL_READY
PREPROCESSING
GENERATING
ENCODING
SAVING
RESULT_DOWNLOADING
COMPLETED
```

실제 진행률을 모델에서 신뢰성 있게 얻을 수 있을 때만 숫자 퍼센트를 표시합니다.

정확한 진행률을 알 수 없는 단계에서는 임의의 퍼센트를 계산하지 않고 단계명과 경과 시간만 표시합니다.

---

## 6. Job 복구 원칙

모든 원격 실행은 가능한 한 `function_call_id`를 저장합니다.

```text
job_id
modal_profile_id
function_call_id
status
stage
created_at
started_at
completed_at
```

앱 재시작 시:

1. SQLite에서 종료되지 않은 Job 조회
2. 저장된 `function_call_id` 확인
3. Python sidecar가 Modal 실행에 재연결
4. 원격 상태 재조회
5. 로컬 상태와 동기화
6. 로그 스트림 재개 또는 결과 다운로드

즉, 데스크톱 앱이 종료되어도 Modal에서 이미 실행 중인 작업 자체는 추적 가능한 구조를 목표로 합니다.

---

## 7. Tauri ↔ Python IPC

프로토콜은 **stdin/stdout JSON Lines(JSONL)** 로 고정합니다.

원칙:

- 한 줄 = 하나의 JSON 메시지
- 모든 메시지에 `type` 포함
- Job 관련 메시지에는 `job_id` 필수
- stdout에는 프로토콜 JSON만 출력
- 일반 Python 디버그 로그는 stderr 사용

### Tauri → Python

#### 작업 실행

```json
{
  "type": "start_job",
  "job_id": "job_20260925_000001",
  "profile_id": "modal_01",
  "input_path": "C:/.../input.png",
  "prompt": "A woman slowly turns her head toward the camera.",
  "options": {
    "duration": 6,
    "resolution": "1080p",
    "seed": null
  }
}
```

#### 기존 작업 재연결

```json
{
  "type": "attach_job",
  "job_id": "job_20260925_000001",
  "profile_id": "modal_01",
  "function_call_id": "fc-..."
}
```

#### 작업 취소

```json
{
  "type": "cancel_job",
  "job_id": "job_20260925_000001"
}
```

### Python → Tauri

#### 실행 ID 확정

```json
{
  "type": "remote_attached",
  "job_id": "job_20260925_000001",
  "function_call_id": "fc-..."
}
```

#### 단계 변경

```json
{
  "type": "stage",
  "job_id": "job_20260925_000001",
  "stage": "MODEL_LOADING"
}
```

#### 진행률

```json
{
  "type": "progress",
  "job_id": "job_20260925_000001",
  "value": 0.43
}
```

#### 로그

```json
{
  "type": "log",
  "job_id": "job_20260925_000001",
  "level": "info",
  "message": "H3 model loaded"
}
```

#### 완료

```json
{
  "type": "completed",
  "job_id": "job_20260925_000001",
  "remote_output_path": "/outputs/job_20260925_000001/output.mp4",
  "local_output_path": "C:/.../results/job_20260925_000001/output.mp4"
}
```

#### 실패

```json
{
  "type": "failed",
  "job_id": "job_20260925_000001",
  "code": "MODAL_EXECUTION_FAILED",
  "message": "Worker exited unexpectedly",
  "retryable": true
}
```

---

## 8. H3 Worker 로그 규약

Modal 로그는 사람이 읽는 로그와 앱이 파싱하는 stage event를 구분합니다.

초기 구현에서는 다음 prefix를 사용합니다.

```text
@@STAGE:CONTAINER_STARTING
@@STAGE:GPU_READY
@@STAGE:MODEL_LOADING
@@STAGE:MODEL_READY
@@STAGE:PREPROCESSING
@@STAGE:GENERATING
@@PROGRESS:0.43
@@STAGE:ENCODING
@@STAGE:SAVING
@@RESULT:/outputs/job_20260925_000001/output.mp4
```

향후 structured event channel을 별도로 만들 수 있지만 MVP에서는 위 규약으로 충분합니다.

---

## 9. Modal Profile Pool

"계정 풀"은 앱 내부에서 **Modal Profile Pool**이라는 이름으로 관리합니다.

각 Profile은 사용자가 접근 권한을 가진 Modal 계정/워크스페이스만 등록하는 것을 전제로 합니다.

### 데이터

```text
id
name
workspace_label
enabled
budget_limit
budget_used
reserve_amount
max_concurrency
running_jobs
priority
last_error
last_used_at
created_at
updated_at
```

Modal credential 원문은 SQLite에 저장하지 않습니다.

SQLite에는 Keychain에서 credential을 찾기 위한 식별자만 저장합니다.

### 스케줄러 선택 조건

후보 Profile은 다음 조건을 만족해야 합니다.

1. `enabled = true`
2. credential 사용 가능
3. `running_jobs < max_concurrency`
4. `budget_limit - budget_used > reserve_amount`
5. cooldown 상태가 아님
6. 치명적인 최근 오류가 없음

후보가 여러 개면 MVP에서는 아래 순서로 선택합니다.

1. priority가 높은 Profile
2. running_jobs가 적은 Profile
3. budget 사용률이 낮은 Profile
4. 가장 오래 사용되지 않은 Profile

스케줄러는 서비스 제한 회피가 아니라 사용자가 권한을 가진 워크스페이스의 정상적인 작업 분배를 목적으로 합니다.

---

## 10. 크레딧 / 예산 표시

GUI에서 표시하는 값은 두 개를 구분합니다.

### 로컬 할당 예산

사용자가 앱에서 직접 정한 Profile별 예산입니다.

예:

```text
Modal 01
할당 예산: $30
사용량: $8.66
가용 예산: $21.34
```

### 관측 사용량

가능한 경우 Modal에서 얻은 billing/usage 데이터를 동기화합니다.

동기화가 불가능하거나 지연되는 경우에는 앱이 기록한 작업별 추정 비용을 별도 표시합니다.

UI에서 "실제 청구 금액"과 "앱 추정치"를 혼동하지 않도록 라벨을 분리합니다.

---

## 11. SQLite 초안

### modal_profiles

```sql
CREATE TABLE modal_profiles (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  workspace_label TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  keychain_ref TEXT NOT NULL,
  budget_limit REAL,
  budget_used REAL NOT NULL DEFAULT 0,
  reserve_amount REAL NOT NULL DEFAULT 0,
  max_concurrency INTEGER NOT NULL DEFAULT 1,
  priority INTEGER NOT NULL DEFAULT 0,
  cooldown_until TEXT,
  last_error TEXT,
  last_used_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

### jobs

```sql
CREATE TABLE jobs (
  id TEXT PRIMARY KEY,
  modal_profile_id TEXT,
  function_call_id TEXT,
  status TEXT NOT NULL,
  stage TEXT NOT NULL,
  prompt TEXT NOT NULL,
  input_path TEXT NOT NULL,
  output_path TEXT,
  thumbnail_path TEXT,
  duration INTEGER,
  resolution TEXT,
  seed INTEGER,
  progress REAL,
  error_code TEXT,
  error_message TEXT,
  retry_count INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  started_at TEXT,
  completed_at TEXT,
  FOREIGN KEY (modal_profile_id) REFERENCES modal_profiles(id)
);
```

### job_events

```sql
CREATE TABLE job_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  job_id TEXT NOT NULL,
  event_type TEXT NOT NULL,
  level TEXT,
  stage TEXT,
  message TEXT,
  payload_json TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY (job_id) REFERENCES jobs(id)
);
```

### usage_records

```sql
CREATE TABLE usage_records (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  modal_profile_id TEXT NOT NULL,
  job_id TEXT,
  source TEXT NOT NULL,
  amount REAL,
  raw_json TEXT,
  observed_at TEXT NOT NULL,
  FOREIGN KEY (modal_profile_id) REFERENCES modal_profiles(id),
  FOREIGN KEY (job_id) REFERENCES jobs(id)
);
```

---

## 12. 파일 저장 구조

기본 로컬 데이터 디렉터리:

```text
Modal GUI/
├─ inputs/
│  └─ <job_id>/
│     └─ input.<ext>
├─ results/
│  └─ YYYY-MM-DD/
│     └─ <job_id>/
│        ├─ output.mp4
│        ├─ thumbnail.jpg
│        └─ metadata.json
├─ logs/
└─ database/
   └─ app.db
```

Modal 측 데이터:

```text
/h3-data/
├─ inputs/
│  └─ <job_id>/
└─ outputs/
   └─ <job_id>/
      ├─ output.mp4
      ├─ thumbnail.jpg
      └─ metadata.json
```

---

## 13. 결과 metadata.json

```json
{
  "job_id": "job_20260925_000001",
  "prompt": "A woman slowly turns her head toward the camera.",
  "profile_id": "modal_01",
  "function_call_id": "fc-...",
  "model": "MiniMax-H3",
  "duration": 6,
  "resolution": "1080p",
  "seed": null,
  "created_at": "2026-09-25T01:00:00+09:00",
  "completed_at": "2026-09-25T01:05:00+09:00"
}
```

---

## 14. 오류 처리

오류는 최소한 다음 카테고리로 정규화합니다.

```text
INVALID_INPUT
NO_AVAILABLE_PROFILE
CREDENTIAL_ERROR
UPLOAD_FAILED
MODAL_SPAWN_FAILED
MODAL_EXECUTION_FAILED
MODEL_LOAD_FAILED
INFERENCE_FAILED
ENCODING_FAILED
DOWNLOAD_FAILED
SIDECAR_CRASHED
UNKNOWN
```

각 오류에는 다음 정보를 저장합니다.

- code
- message
- retryable
- raw error
- 발생 stage
- 발생 timestamp

재시도는 자동 무한 반복하지 않습니다.

MVP 기본값:

- 네트워크/다운로드 계열: 최대 2회
- 모델/입력 계열: 자동 재시도 없음
- 사용자가 언제든 수동 재시도 가능

---

## 15. 앱 재실행 시 복구

시작 시 Rust Core가 다음 순서로 복구합니다.

```text
SQLite open
   ↓
RUNNING / ASSIGNING / DOWNLOADING Job 조회
   ↓
Python sidecar 시작
   ↓
function_call_id가 있으면 attach 시도
   ↓
원격 상태와 로컬 상태 reconcile
   ↓
완료된 원격 작업이면 결과 다운로드
   ↓
실패/소실된 작업이면 FAILED로 정규화
```

---

## 16. 보안 원칙

- Modal token/secret을 Git repository에 저장하지 않음
- Modal token/secret을 SQLite 평문으로 저장하지 않음
- OS Keychain/Credential Manager 사용
- 로그에 credential 출력 금지
- Python sidecar 실행 환경에 필요한 credential만 주입
- Profile 삭제 시 Keychain credential도 함께 제거 가능하도록 설계
- 결과물/입력 이미지는 기본적으로 로컬 보관
- 사용자가 명시하지 않는 외부 분석/업로드 기능은 넣지 않음

---

## 17. 프론트엔드 화면 구조

Figma와 실제 구현에서 동일한 정보 구조를 사용합니다.

### Generate

- 이미지 Drop Zone
- Prompt textarea
- Duration
- Resolution
- Seed
- Generate 버튼
- 최근 생성 preset

### Jobs

- Queue
- Running
- Completed
- Failed
- 상태 필터
- Profile 필터
- 생성 시간
- 경과 시간

### Job Detail

- 입력 이미지
- 결과 플레이어
- Prompt
- Profile
- 상위 상태
- 현재 stage
- 진행률
- 경과 시간
- 단계 Timeline
- 로그
- Retry
- Cancel
- 결과 폴더 열기

### Modal Pool

- Profile 이름
- Enabled 상태
- Workspace label
- 할당 예산
- 관측 사용량
- 남은 로컬 예산
- Running jobs
- Max concurrency
- 최근 오류
- Enable / Disable

### Results

- 썸네일 Grid
- 영상 플레이어
- Prompt
- 생성 옵션
- 생성 시각
- 사용 Profile
- 폴더 열기
- 동일 설정으로 재생성

### Settings

- 기본 결과 저장 경로
- Modal Profile 관리
- Profile별 max concurrency
- Profile별 local budget
- 기본 H3 생성 옵션
- 알림 설정
- 로그 보관 기간

---

## 18. 프로젝트 디렉터리 목표

```text
modal-gui/
├─ src/
│  ├─ pages/
│  │  ├─ Generate.tsx
│  │  ├─ Jobs.tsx
│  │  ├─ JobDetail.tsx
│  │  ├─ ModalPool.tsx
│  │  ├─ Results.tsx
│  │  └─ Settings.tsx
│  ├─ components/
│  ├─ stores/
│  ├─ types/
│  └─ lib/
│
├─ src-tauri/
│  ├─ src/
│  │  ├─ commands/
│  │  ├─ database/
│  │  ├─ jobs/
│  │  ├─ scheduler/
│  │  ├─ sidecar/
│  │  └─ results/
│  └─ binaries/
│
├─ worker/
│  ├─ main.py
│  ├─ protocol.py
│  ├─ modal_client.py
│  ├─ job_runner.py
│  └─ profile.py
│
├─ modal/
│  ├─ app.py
│  ├─ h3_worker.py
│  ├─ inference.py
│  └─ model_loader.py
│
└─ README.md
```

---

## 19. MVP 범위

### 포함

- Tauri 데스크톱 앱
- 이미지 1장 입력
- 프롬프트 입력
- H3 영상 생성
- Modal Profile 등록
- Profile enable/disable
- Job queue
- 실시간 stage 표시
- 로그 표시
- 결과 MP4 자동 다운로드
- 앱 내 영상 재생
- 로컬 결과 폴더
- 앱 재시작 후 Job 복구
- 실패 작업 수동 재시도

### MVP 이후

- Batch 생성
- 여러 이미지 입력
- Prompt preset
- 다양한 모델 adapter
- 작업 우선순위
- 작업별 비용 분석
- Profile 자동 cooldown
- 고급 queue 정책
- 결과 비교 UI
- FFmpeg 후처리 preset

---

## 20. MVP에서 하지 않는 것

- MiniMax 상용 API 호출
- 브라우저 자동화로 Modal 웹 UI 조작
- Modal 웹사이트 DOM scraping
- 비밀번호/브라우저 cookie 저장
- 임의 진행률 추정
- 무한 자동 재시도
- credential을 프론트엔드 상태에 노출
- 서비스 제한이나 과금 정책을 우회하기 위한 계정 로테이션

---

## 21. 구현 순서

### Phase 1 — End-to-End 최소 경로

```text
이미지 + Prompt
→ Rust Job 생성
→ Python sidecar
→ Modal Function
→ H3 inference
→ MP4
→ 자동 다운로드
→ 앱 플레이어
```

### Phase 2 — 관찰 가능성

- stage event
- 실시간 로그
- Job Detail
- 실패 이유
- elapsed time

### Phase 3 — Pool / Queue

- Modal Profile CRUD
- max concurrency
- local budget
- scheduler
- queue

### Phase 4 — 복구 / 안정성

- FunctionCall reconnect
- app restart recovery
- sidecar crash recovery
- retry policy

### Phase 5 — UX

- Results gallery
- preset
- desktop notification
- 폴더 열기
- 재생성

---

## 22. 제품 원칙

1. **웹 UI를 찾아다니지 않는다.** 모든 핵심 상태와 결과는 데스크톱 앱에 노출한다.
2. **실제 상태만 표시한다.** 알 수 없는 진행률을 만들어내지 않는다.
3. **작업은 복구 가능해야 한다.** 앱 재시작이 원격 작업 추적 손실로 이어지지 않게 한다.
4. **credential은 UI/DB에서 분리한다.**
5. **Rust Core가 상태의 Source of Truth다.**
6. **Python은 Modal adapter로 제한한다.**
7. **H3 Worker는 영상 생성 책임에 집중한다.**
8. **백엔드 프로토콜을 먼저 고정하고 UI는 그 상태 모델을 그대로 반영한다.**

---

## 23. 현재 결정 상태

이 README의 백엔드 구조를 **v0 아키텍처 기준선**으로 사용합니다.

구현 과정에서 변경이 필요한 경우에는 코드가 문서와 조용히 어긋나게 두지 않고, 아키텍처 변경을 먼저 문서에 반영한 뒤 구현합니다.

다음 단계는 이 상태 모델을 기준으로 Figma에서 데스크톱 UI의 정보 구조와 주요 화면을 확정하는 것입니다.
