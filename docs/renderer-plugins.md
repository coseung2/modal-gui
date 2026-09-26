# 렌더러 플러그인

모션그래픽 출력은 플러그인으로 갈아끼웁니다. 파이프라인은 소재 준비(클립 선택, 비트 검출,
컷 길이 계산, 타이포 큐)까지만 담당하고, 실제 그림을 만드는 일은 플러그인이 합니다. 새 도구를
붙일 때 프런트엔드는 건드리지 않습니다.

## 실행 모드

도구마다 자동화 수준이 달라서 두 가지로 나눕니다. 이 구분이 중요한 이유는, 무인 렌더가 안 되는
도구에 진행률 바를 붙이면 앱이 거짓 정보를 표시하기 때문입니다.

| 모드 | 의미 | 앱 동작 |
| --- | --- | --- |
| `batch` | 프로세스를 띄워 완성 파일을 받습니다 | 진행률 %, `render_completed`, 인라인 프리뷰 |
| `project_handoff` | 프로젝트/스크립트만 준비하고 출력은 사용자가 도구에서 실행합니다 | 진행률 없음, `prepared`, 스크립트 열기 버튼 |

## 현재 플러그인

### FFmpeg (`ffmpeg`, batch)

항상 쓸 수 있는 기준선이고, 지금은 비트 반응형 키네틱 타이포까지 합니다. 큐마다 등장
애니메이션이 네 종류(슬라이드, 라이즈, 와이프, 펀치) 중 하나로 돌아가고, 검출된 비트마다
화면이 살짝 확대되고 글자가 튀며 밝아집니다.

FFmpeg 표현식으로 할 수 있는 것과 없는 것이 명확히 갈리는데, 이 빌드(8.1.1)에서 직접
확인한 결과는 이렇습니다.

| 대상 | 표현식 | 쓰는 곳 |
| --- | --- | --- |
| `scale` w/h | 가능 | 비트마다 화면 확대 |
| `drawtext` x/y | 가능 | 슬라이드, 드롭, 비트 지터 |
| `drawtext` alpha | 가능 | 페이드, 비트 밝기 |
| `drawbox` w | 가능 | 와이프 마스크, 진행 바 |
| `drawtext` fontsize | **쓰지 말 것** | 아래 참고 |
| `rgbashift` | 불가 | — |
| `blend` all_expr | **쓰지 말 것** | 아래 참고 |

함정이 두 개 있어서 적어둡니다. 둘 다 오류 메시지 없이 렌더가 실패합니다.

`fontsize`에 표현식을 넣으면 파싱은 되고 작은 합성 입력에서는 동작하지만, 실제 1080p
클립에서는 drawtext가 멈추고 프레임이 0개 나옵니다. 그래서 글자 크기는 고정하고 비트
반응은 위치와 투명도로 처리합니다.

`blend`의 `all_expr`은 프레임이 아니라 **픽셀마다** 평가됩니다. 여기에 비트 표현식을 넣으니
CPU 37분을 태우고도 프레임 하나를 못 뽑았습니다. 비트 값은 프레임 단위 컨텍스트에서만 씁니다.

비트 표현식 자체도 형태가 중요합니다. 비트당 펄스를 더하는 평탄한 합은 47개 비트에서 41KB
그래프가 되어 커맨드라인 한계를 넘고 매 프레임 47개 항을 모두 계산합니다. 지금은 시간으로
분기하는 `if` 트리를 만들어 한 펄스만 도달하게 하고, 그래프는 `-filter_complex_script`로
파일 전달합니다. 60초 결과물이 32초에 렌더됩니다.

### Autograph (`autograph`, batch)

무인 렌더를 지원하는 유일한 GUI 도구입니다.

```
AutographRenderer.exe project.agp --background --no-splashscreen \
  --render Main output_file=out.mov format=1920x1080:1.0 \
  framerate=24.0 range=0:00:00:00;0:00:01:00 \
  container=mov video_codec=prores
```

설치된 빌드의 `AutographRenderer.exe --help`에서 확인한 문법입니다. 문서와 다른 점이 세 가지
있습니다. 배치 전용 바이너리는 `AutographRenderer.exe`이고, `range`는 `H:MM:SS:FF` 두 개를
`;`로 잇고, 설치 경로에 버전이 붙습니다(`C:\Program Files\Maxon Autograph 2026\bin`).

프로젝트에 지정한 컴포지션이 Render Manager에 없으면 Autograph이 무한 대기합니다. 그래서
플러그인이 타임아웃을 걸고 어떤 컴포지션을 못 찾았는지 알려줍니다. 기본값은 300초 +
영상 길이×30초이고 `--renderer-option timeout=<초>`로 바꿉니다.

두 가지 조건이 있습니다.

첫째, Autograph은 **Maxon App 카탈로그에 없습니다.** `mx1 product list`에 나오지 않고
`mx1 package query autograph`도 `Product not found`를 반환합니다. maxon.net에서 받는 별도
인스톨러(약 1GB)가 유일한 설치 경로이고, 그 인스톨러는 권한 상승을 요구합니다.

둘째, 라이선스입니다. 설치된 2026.1 v0은 `--version`과 `--help`가 정상 동작하지만,
`mx1 license list`에는 Autograph 항목이 없습니다(현재 Maxon One Trial만 잡힙니다).
실제 렌더가 허용되는지는 `.agp` 템플릿으로 한 번 렌더해봐야 확정됩니다.

설치는 도우미 스크립트로 합니다. UAC 승인이 한 번 필요합니다.

```powershell
pwsh -File tools\install_autograph.ps1
```

인스톨러가 이미 `.tooling`에 있으면 다시 받지 않고 바로 실행합니다. 설치가 끝나면 스크립트가
`Autograph.exe` 경로와 라이선스 확인 명령을 알려줍니다.

템플릿이 없으면 무인 렌더를 건너뛰고 소재 데이터만 사이드카 JSON으로 내보낸 뒤 `prepared`를
보고합니다. 하드 실패로 끝내지 않는 쪽이 쓸모가 있기 때문입니다.

컴포지션 파라미터를 렌더마다 덮어쓰는 구조라 `.agp` 템플릿이 필요합니다. 우리 데이터는
`<output>.autograph-input.json` 사이드카로 전달하고, 템플릿이 그 경로를 읽습니다.

```
--renderer autograph --renderer-option template=F:\...\trailer.agp \
  --renderer-option composition=Main --renderer-option video_codec=prores
```

### Cavalry (`cavalry`, project_handoff)

Cavalry는 2.7에서 CLI가 제거됐고 헤드리스 렌더가 Enterprise 기능입니다. 그래서 완성 파일을
약속하지 않습니다. 대신 샷 레이아웃, 타이포 레이어, 비트 마커, 오디오 레이어, 렌더 큐 항목까지
만드는 JavaScript 씬 빌더를 생성하고 `%APPDATA%\Cavalry\Third-Party\Plugins`에 설치합니다.
사용자는 Cavalry Script Editor에서 실행한 뒤 Render Queue로 출력합니다.

생성 스크립트는 Cavalry가 설치와 함께 제공하는
`assets/MetaData/api_function_metadata.json`에 실재하는 함수만 씁니다. `createComp`,
`setActiveComp`, `set`, `loadAsset`, `addAssetToComp`, `create`, `setInFrame`,
`setOutFrame`, `createTimeMarker`, `addRenderQueueItem`, `readFromFile`입니다.
CLI가 없어서 오타가 사람이 실행할 때까지 드러나지 않으므로, 테스트가 생성된 스크립트의
모든 `api.*` 호출을 그 메타데이터와 대조합니다. Cavalry가 없는 호스트에서는 건너뜁니다.

무료 티어는 Canva 계정 로그인이 필요하지만, 2.7부터 해상도 상한과 워터마크가 없습니다.

## 새 플러그인 추가하기

`tools/renderers/<name>_renderer.py`에 클래스를 하나 만들고 `__init__.py`의 `_PLUGINS`에 넣으면
끝입니다. GUI는 레지스트리가 보고하는 것을 그대로 표시합니다.

```python
class MyRenderer:
    id = "mytool"
    name = "My Tool"
    execution = "batch"  # 또는 "project_handoff"

    def describe(self) -> PluginInfo:
        """호스트를 실제로 탐지해서 available 여부와 능력을 보고합니다.
        설치돼 있지 않으면 unavailable_reason에 구체적인 이유를 적습니다."""

    def render(self, request: RenderRequest, emit: Emit) -> Path:
        """batch는 완성 파일을, project_handoff는 프로젝트 파일 경로를 반환합니다."""
```

지킬 것이 두 가지 있습니다. `describe()`는 추측하지 말고 실제로 실행 파일을 찾아야 하고,
`project_handoff` 플러그인은 `render_progress`나 `render_completed`를 보내면 안 됩니다.
`prepared`만 보냅니다.

## 이벤트

stdout에 JSON 한 줄씩 씁니다. Tauri가 읽어 `pipeline-event`로 프런트엔드에 전달합니다.

| 이벤트 | 보내는 쪽 | 뜻 |
| --- | --- | --- |
| `render_started` | batch | 렌더 시작, 샷/비트 수 포함 |
| `render_progress` | batch | `percent`, `seconds` |
| `render_completed` | batch | 완성 파일 경로와 실측 길이 |
| `prepare_started` | handoff | 프로젝트 준비 시작 |
| `prepared` | handoff | 스크립트/프로젝트 경로와 다음 할 일 안내 |
| `failed` | 양쪽 | `message`, 필요하면 `detail` |

## 확인 방법

```powershell
python tools\motion_graphics_pipeline.py plugins
```

설치 상태와 능력이 JSON으로 나옵니다. 앱의 출력 섹션 카드도 같은 데이터를 씁니다.
