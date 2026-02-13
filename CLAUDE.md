# CLAUDE.md

이 파일은 Claude Code가 hwpx-rust 프로젝트를 이해하는 데 필요한 컨텍스트를 제공합니다.

## 프로젝트 개요

HWP/HWPX 문서를 파싱하고 Markdown, HTML, JSON으로 변환하는 Rust 라이브러리입니다.
Python 바인딩(PyO3)을 통해 Python에서도 사용 가능합니다.

**HWP 바이너리 지원은 동결(frozen) 상태** — 향후 개발은 HWPX(XML) 파서에 집중합니다.

## 핵심 아키텍처

```
hwpx-rust/
├── crates/hwp-core/          # Rust 핵심 라이브러리
│   └── src/
│       ├── parser/           # 파일 파싱
│       │   ├── detect.rs     # HWP/HWPX 형식 자동 감지
│       │   └── hwpx/         # HWPX(XML) 파서
│       │       ├── section.rs # 섹션 파싱 (문단, 테이블, 이미지, 하이퍼링크)
│       │       ├── header.rs  # 문서 정보 파싱 (폰트, 스타일, 문단 모양)
│       │       └── bindata.rs # 바이너리 데이터 파싱
│       │
│       ├── document/         # 문서 구조체 정의
│       │   ├── bodytext/     # 본문 (섹션, 문단, 표, 그림)
│       │   ├── docinfo/      # 문서 정보 (폰트, 스타일)
│       │   └── bindata/      # 바이너리 데이터 (이미지)
│       │
│       ├── error.rs          # 에러 타입 + ParseWarnings 시스템
│       │
│       └── viewer/           # 출력 변환기
│           ├── shared.rs     # 공통 유틸리티 (MIME 감지, 이미지 저장)
│           ├── markdown/     # Markdown 변환
│           │   ├── mod.rs    # MarkdownOptions (Default impl, image_alt_text)
│           │   ├── common.rs # 이미지 포맷팅 (shared.rs 활용)
│           │   └── document/bodytext/
│           │       ├── table.rs     # 테이블 렌더링
│           │       ├── paragraph.rs # 문단 변환 (2개 함수 — 아래 주의사항 참조)
│           │       └── para_text.rs # 텍스트 스타일 (bold, italic, strikethrough, underline)
│           └── html/         # HTML 변환
│               ├── document.rs # HTML 문서 구조 (charset 선언)
│               ├── text.rs     # 텍스트 렌더링 (XSS 방지 html_escape)
│               └── common.rs   # 이미지 URL 생성 (shared.rs 활용)
│
└── packages/hwpx-python/     # Python 바인딩 (PyO3)
```

## 주요 파일

### 파서 (Parser)
- `src/parser/hwpx/section.rs`: HWPX 섹션 파싱
  - 중첩 테이블 지원 (`TableState` 스택으로 부모 테이블 상태 저장)
  - `table_depth` 추적으로 테이블 중첩 레벨 관리
  - 테이블 셀 내 이미지 파싱
  - 하이퍼링크 파싱 (`fieldBegin`/`fieldEnd`)
  - 문단 속성 파싱 (`prIDRef`, `styleIDRef`)
  - **20개 유닛 테스트** (`#[cfg(test)] mod tests` — XML 조각 기반)

### Markdown 변환
- `src/viewer/markdown/mod.rs`: `to_markdown()` 진입점
  - `convert_bodytext_to_markdown()` 직접 호출 (unsafe `process_bodytext()` 우회)
  - `MarkdownOptions` — `Default` impl, `image_alt_text: Option<String>` 필드
- `src/viewer/markdown/document/bodytext/table.rs`: 테이블 렌더링
- `src/viewer/markdown/document/bodytext/para_text.rs`: 텍스트 스타일링
  - `apply_markdown_styles()` — bold, italic, strikethrough, **underline** 지원

### 공통 유틸리티
- `src/viewer/shared.rs`: HTML/Markdown 뷰어 간 공유 함수
  - `detect_mime_type_from_base64()` — PNG, JPEG, BMP, GIF, WebP, TIFF
  - `get_extension_from_bindata_id()`, `get_mime_type_from_bindata_id()`
  - `save_image_to_file()`

### Python 바인딩
- `packages/hwpx-python/src/lib.rs`: PyO3 바인딩
  - `parse()`, `parse_file()`: 파싱 함수
  - `Document.to_markdown()`, `to_html()`, `to_json()`, `get_text()`
  - `Document.warnings`: 파싱 경고 목록

## 빌드 명령어

```bash
# Rust 빌드
cargo build --release

# 전체 테스트
cargo test --workspace

# 스냅샷 테스트 업데이트 (변경 후)
cargo insta accept --workspace

# Clippy 린트
cargo clippy --all-targets --all-features

# Python 휠 빌드 (maturin 사용)
cd packages/hwpx-python
maturin build --release
```

## 개발 참고사항

### 파일 형식
- **HWP 5.0**: Compound File Binary Format (CFB) — **동결 상태**
- **HWPX**: ZIP 내부 XML 파일들 (OWPML 표준)
  - `Contents/section0.xml`: 본문 섹션
  - `Contents/header.xml`: 문서 설정 (폰트, 스타일)
  - `BinData/`: 이미지 등 바이너리 데이터

### 테이블 구조 (HWPX)
```xml
<hp:tbl>           <!-- 테이블 -->
  <hp:caption>     <!-- 캡션 (선택) -->
  <hp:tr>          <!-- 행 -->
    <hp:tc>        <!-- 셀 -->
      <hp:cellAddr colAddr="0" rowAddr="0"/>
      <hp:cellSpan colSpan="2" rowSpan="1"/>
      <hp:subList> <!-- 셀 내용 컨테이너 -->
        <hp:p>     <!-- 문단 -->
          <hp:pic> <!-- 이미지 -->
          <hp:tbl> <!-- 중첩 테이블 -->
```

### 주의사항
- 중첩 테이블은 `table_state_stack`으로 부모 상태 저장/복원
- 테이블 셀 내 이미지는 `CellContentItem::Image`로 관리
- Markdown 출력 시 테이블은 HTML `<table>` 태그 사용
- `paragraph.rs`에 2개의 유사 함수 존재 (`convert_paragraph_to_markdown`과 `_with_state`) — 테이블/이미지 처리가 **분기됨**. 하나를 다른 것의 래퍼로 만들지 말 것
- `MarkdownOptions`는 직접 구조체 리터럴로 사용되는 곳이 많음 — 새 필드 추가 시 모든 사이트에 `None` 추가 필요
- `section.rs`의 `trim_text(true)` 설정으로 텍스트 노드 앞뒤 공백이 제거됨
- `section.rs`의 탭 텍스트는 `current_text`에만 추가 (runs에는 미포함)
- `HwpDocument.warnings` 필드로 파싱 경고 추적 가능
- `panic = "unwind"` — PyO3 호환성을 위해 abort 대신 unwind 사용
- 스냅샷 테스트는 `insta` 크레이트 사용 — `cargo insta accept --workspace`로 업데이트

### CI (`.github/workflows/ci.yml`)
- `Swatinem/rust-cache@v2`로 빌드 캐싱
- `cargo audit`로 보안 취약점 검사
- `feature/**` 브랜치에서도 CI 실행
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
