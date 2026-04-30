# wie_libretro

[WIE](https://github.com/dlunch/wie) (WIPI / SKVM / J2ME 에뮬레이터) 의 libretro
프론트엔드. RetroArch 코어 (`.dll` / `.so` / `.dylib`) 로 빌드되어 RetroArch가
지원하는 모든 플랫폼에서 동작한다.

## 빌드

워크스페이스 루트(이 README의 한 단계 위)에서:

```bash
# Windows에서는 LIBCLANG_PATH 환경변수 필요 (rust-libretro-sys가 bindgen 사용)
LIBCLANG_PATH="C:\\Program Files\\LLVM\\bin" \
  cargo build --release -p wie_libretro
```

`dev` 빌드는 큰 게임에서 인터프리터 stack frame이 더 커지면서 충돌이
훨씬 잘 난다. **항상 `--release` 로 빌드한다.**

산출물 (플랫폼별):

| OS | 경로 |
|---|---|
| Windows | `target/release/wie_libretro.dll` |
| Linux | `target/release/libwie_libretro.so` |
| macOS | `target/release/libwie_libretro.dylib` |

## 설치

빌드한 코어 라이브러리를 RetroArch의 `cores/` 디렉토리로 복사하면 끝.

```bash
# Windows 예시
cp target/release/wie_libretro.dll "C:/RetroArch-Win64/cores/"
```

RA 메뉴 → **Load Core** → **wie**.

### 시스템 디렉토리 / 폰트

코어는 RA의 system directory를 받아 `<system>/wie/<aid>/{db,fs}/` 하위에
게임별 데이터를 저장한다. 별도 폰트 파일은 필요 없다 (코어에 내장).

RA 가 system directory를 노출하지 않는 경우 ProjectDirs로 fallback:

| OS | 경로 |
|---|---|
| Windows | `%APPDATA%\dlunch\wie\data\` |
| Linux | `~/.local/share/wie/` |
| macOS | `~/Library/Application Support/net.dlunch.wie/` |

## 컨텐츠 형식

지원 확장자: `.jar`, `.zip`, `.kjx`, `.wie`

KTF / LGT / SKT 게임은 멀티-파일 archive 형태 (`.jar` + `__adf__` 등)이다.
이걸 `.zip` 그대로 쓰면 **RetroArch가 archive로 자동 인식해서 내부 `.jar`만
코어에 넘겨버린다** — 이러면 archive 메타가 사라져서 부팅 안 된다. 우회:

```bash
# 그냥 확장자만 바꾸면 됨. 내용물은 zip 그대로.
mv 게임.zip 게임.kjx
```

J2ME `.jar` 단독 파일은 그대로 쓰면 된다.

## 입력 매핑 (RetroPad → WIPI/J2ME)

| RetroPad | KeyCode |
|---|---|
| D-Pad ↑↓←→ | `UP` / `DOWN` / `LEFT` / `RIGHT` |
| A | `OK` |
| B | `CLEAR` |
| X | `HASH` (`#`) |
| Y | `NUM0` |
| L | `LEFT_SOFT_KEY` |
| R | `RIGHT_SOFT_KEY` |
| Start | `CALL` |
| Select | `HANGUP` |
| L2 / R2 | `NUM1` / `NUM3` |
| L3 / R3 | `NUM7` / `NUM9` |

## 알려진 한계

### 1. 세이브스테이트 비지원

`wie_backend::Emulator` trait에 serialize / deserialize 메서드가 없다. 본체에
추가될 때까지 RA의 세이브스테이트는 동작하지 않는다.

### 2. MIDI는 무음

WIE의 MIDI 콜백 (`midi_note_on/off/program_change/control_change`) 은
호스트 MIDI 디바이스로 직접 송출되도록 설계되어 있다. libretro는 PCM batch
경로만 받으므로, 코어 내부에 소프트신스가 추가되기 전까지는 MIDI BGM이
무음으로 처리된다. PCM (`play_wave`) 효과음은 정상.

### 3. 픽셀 포맷

XRGB8888 고정. RGB565로 폴백하지 않는다.

## 개발자 노트

- `rust-libretro-sys 0.3.2` 의 bindgen 출력에서 `retro_game_info`가 1바이트
  opaque struct (`_address: u8`) 로 잘못 생성된다. 이를 우회하려고
  `retro_load_game` 안에서 `RETRO_ENVIRONMENT_GET_GAME_INFO_EXT` 를 직접
  호출해 `retro_game_info_ext` 에서 path/data를 꺼낸다. 이 env command를
  지원하지 않는 구버전 RetroArch에서는 동작하지 않는다.
- `dev` profile은 `wie_core_arm` / `arm32_cpu` 의 stack frame이 release보다
  훨씬 커서 helloworld도 죽일 수 있다. 항상 `--release`.
- WIE의 ARM 인터프리터 + JVM 호출 chain은 깊어서 Win64의 1MB 기본
  main-thread stack을 쉽게 넘는다. `tick()` 과 `build_emulator` 호출을
  `stacker::maybe_grow` 로 감싸서 stack이 부족할 때 32MB worker thread로
  자동 이양한다. EXE의 stack reserve와 무관하게 동작하므로 RetroArch 본체
  수정 불필요.

## 라이선스

MIT (워크스페이스 본체와 동일).
