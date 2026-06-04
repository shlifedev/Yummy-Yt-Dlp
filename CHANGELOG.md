# Changelog


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
