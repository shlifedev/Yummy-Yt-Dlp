# Changelog


## v1.1.51 (2026-06-15)

### Bug Fixes
- align tauri rust dependency versions

## v1.1.50 (2026-06-14)

### Features
- add Linux to the build and release pipeline

### Bug Fixes
- expose yt-dlp's private sonames so linuxdeploy can resolve them
- install libtinfo5 so linuxdeploy can resolve yt-dlp's readline dep
- skip linuxdeploy stripping so the AppImage bundles
- satisfy windows clippy in reserved-name path check
- surface silent frontend failures and clean up listeners

### Other Changes
- verbose linux dev bundling and drop rpm from dev builds

## v1.1.49 (2026-06-11)

## v1.1.48 (2026-06-11)

### Features
- stream playlist/channel scans instead of 50-entry pagination
- fetch sidecar binaries automatically before tauri dev

### Bug Fixes
- eliminate download queue races, stuck 'downloading' rows, and bad history records
- stop duplicate-warning self-destruct and silent scan/batch state loss
- surface download, queue-action, and updater failures in the UI
- skip undownloadable id-only entries and invalid batch items instead of failing the whole enqueue
- surface settings save failures and stop stale-snapshot overwrites
- recover corrupt settings.json, clamp sleep interval, detect per-user browsers
- atomic dependency installs, network timeouts, verified binaries, and download-aware updates
- classify yt-dlp failures correctly and localize stored error messages
- kill yt-dlp on exit and timeouts, add single-instance guard, stop process leaks
- survive corrupt databases and interrupted migrations, bound queue growth
- keep binaries/ytdlp dir for build-time resource validation

### Other Changes
- retry binary downloads and add BtbN autobuild fallback for ffmpeg
- fix action build failures
- add dev-only build workflow publishing a rolling dev pre-release
- remove release deployment internals and roadmap from READMEs

## v1.1.47 (2026-06-07)

## v1.1.46 (2026-06-07)

## v1.1.45 (2026-06-07)

### Features
- show channel auto-loading status

### Bug Fixes
- refresh tauri updater signing key
- satisfy rust lint gates
- sync bun lockfile
- reject zero rate limits
- validate download section times
- reject invalid proxy ports
- skip thumbnails without video ids
- ignore stale log loads
- ignore stale playlist responses
- allow duplicate download override
- 메인 페이지 분석/다운로드 상태 버그 정리

### Other Changes
- release windows and macos only
- deploy updater artifacts to r2
- 릴리스 빌드에 lint/타입 검사 게이트 추가 + release 프로파일 최적화

## v1.1.44 (2026-06-04)

## v1.1.43 (2026-06-03)

## v1.1.42 (2026-06-03)

### Features
- 플레이리스트/채널 배치 다운로드를 그룹으로 묶어 표시

### Other Changes
- 의존성 셋업 설명을 번들 방식에 맞게 수정

## v1.1.41 (2026-06-03)

## v1.1.40 (2026-06-03)

## v1.1.39 (2026-06-03)

## v1.1.38 (2026-06-03)

## v1.1.37 (2026-06-03)

## v1.1.36 (2026-06-03)

### Features
- macOS 타이틀바를 투명 오버레이로 변경
- 의존성 자동 업데이트 옵션 추가
- 의존성별 번들/시스템 소스 선택 토글 추가

### Bug Fixes
- 의존성 탭 동시 설치/업데이트 동시성 버그 수정 (#42)
- 사용자 영향 버그 다수 수정 (#41)
- Windows clippy 실패 유발하던 unused CommandExt import 제거

### Performance
- SQLite WAL 튜닝 + 진행률 DB 쓰기 스로틀, 종료코드 분류 테스트화 (#40)

### Other Changes
- 코드에서 참조되지 않는 i18n 키 45개 제거 (#38)
- CLAUDE.md를 실제 모듈/라우트 구조에 맞게 갱신 (#33)

## v1.1.35 (2026-06-02)

## v1.1.34 (2026-04-04)

## v1.1.33 (2026-03-24)

## v1.1.32 (2026-02-18)

### Features
- skip youtube oembed for non-youtube urls

## v1.1.31 (2026-02-18)

## v1.1.30 (2026-02-15)

## v1.1.29 (2026-02-15)

## v1.1.28 (2026-02-15)

### Features
- add automatic changelog generation to release workflow

### Bug Fixes
- harden Rust backend security and fix critical bugs
- improve error handling and safety across backend
- prevent stale DB connections after factory reset
- add missing CommandExt imports for Windows builds

### Refactoring
- remove unused AppState and increment_counter command
- split large Rust backend files into single-responsibility modules

## v1.1.27 (2026-02-15)

### Features
- add automatic changelog generation to release workflow

### Refactoring
- split large Rust backend files into single-responsibility modules
All notable changes to this project will be documented in this file.
