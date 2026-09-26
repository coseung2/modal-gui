# Autograph .agp 템플릿 명세

Autograph 배치 렌더는 프로젝트를 코드로 만들지 않고, 미리 만들어 둔 `.agp` 템플릿의 컴포지션
파라미터를 렌더마다 덮어쓰는 방식입니다. 그래서 템플릿 쪽에 약속된 이름이 있어야 합니다.
이 문서는 파이프라인이 무엇을 넘기는지와 템플릿이 무엇을 읽어야 하는지를 정합니다.

## 파이프라인이 넘기는 것

커맨드라인 인자로 출력 설정이 들어갑니다.

```
AutographRenderer.exe trailer.agp --background --no-splashscreen --render Main \
  output_file=F:\modal-gui\deliverables\out.mov \
  format=1920x1080:1.0 framerate=24.0 \
  range=0:00:00:00;0:01:00:00 \
  container=mov video_codec=prores \
  input_json=F:\modal-gui\deliverables\out.autograph-input.json
```

`range`는 `H:MM:SS:FF` 타임코드 두 개를 `;`로 잇습니다. 플러그인이 초를 이 형식으로
변환하고, 테스트가 변환 결과를 고정합니다.

클립 목록, 컷 길이, 비트, 타이포 큐는 인자로 넘기기에 양이 많아서 사이드카 JSON으로 보냅니다.
경로는 `input_json` 파라미터로 전달됩니다.

```json
{
  "clips": ["F:/modal-gui/h3-clips/generated/fl2v_00002-audio.mp4", "..."],
  "audio": "F:/modal-gui/music/yue2/.../audio-60s.flac",
  "shot_durations": [5.5, 5.7, 6.35, 6.45, 6.0, 6.0, 6.45, 6.0, 6.0, 5.55],
  "beats": [0.55, 1.25, 4.65, "..."],
  "cues": [{ "start": 0.0, "end": 5.0, "text": "PUBG: BATTLEGROUNDS", "size": 86 }]
}
```

`shot_durations`는 비트에 스냅된 실제 컷 길이입니다. 합계가 `range`의 길이와 같습니다.
`beats`는 초 단위 타임스탬프이고, 전환이나 플래시를 붙일 지점입니다.

## 템플릿이 갖춰야 할 것

컴포지션 이름은 `Main`으로 둡니다. `--render` 인자의 기본값이고, 바꾸려면
`--renderer-option composition=<이름>`으로 넘깁니다.

이 이름이 Render Manager에 실제로 있어야 합니다. 없으면 Autograph이 오류를 내지 않고
무한 대기합니다. 플러그인이 타임아웃으로 끊고 어떤 이름을 못 찾았는지 알려주지만,
렌더는 실패로 끝납니다.

파라미터에 스크립트 이름을 붙여야 커맨드라인에서 덮어쓸 수 있습니다. 최소한 `input_json`
하나는 문자열 파라미터로 노출해야 합니다. 템플릿 스크립트가 그 경로를 읽어 레이어를
구성합니다.

템플릿이 해야 할 일은 네 가지입니다. 사이드카의 `clips`와 `shot_durations`로 푸티지 레이어를
순서대로 배치하고, `audio`를 오디오 트랙에 걸고, `cues`로 타이포 레이어를 만들고, `beats`를
마커나 전환 트리거로 쓰는 것입니다.

## 왜 템플릿을 코드로 안 만드는가

`.agp`는 Autograph 고유 포맷이고 공개 스펙이 없습니다. 추측으로 바이너리를 합성하면 버전이
올라갈 때 조용히 깨집니다. Autograph 안에서 한 번 만들어 두고 파라미터만 바꾸는 쪽이 안전합니다.

## 템플릿 없이 실행하면

무인 렌더를 건너뛰고 사이드카 JSON만 내보낸 뒤 `prepared`를 보고합니다. 하드 실패로 끝내지
않는 이유는, 데이터 자체는 수동으로 프로젝트에 연결할 때 그대로 쓸 수 있기 때문입니다.

```powershell
python tools\motion_graphics_pipeline.py render --renderer autograph `
  --renderer-option template=F:\modal-gui\templates\trailer.agp `
  --mode graphics --audio <오디오> --clips-root <클립폴더> `
  --output F:\modal-gui\deliverables\out.mov --snap-cuts
```
