# CLAUDE.md

이 파일은 Claude Code가 hwpx-rust 프로젝트를 이해하는 데 필요한 컨텍스트를 제공합니다.

## 프로젝트 개요

HWP/HWPX 문서를 파싱하고 Markdown, HTML, JSON으로 변환하는 Rust 라이브러리입니다.
Python 바인딩(PyO3)을 통해 Python에서도 사용 가능합니다.

## 핵심 아키텍처

```
hwpx-rust/
├── crates/hwp-core/          # Rust 핵심 라이브러리
│   └── src/
│       ├── parser/           # 파일 파싱
│       │   ├── detect.rs     # HWP/HWPX 형식 자동 감지
│       │   └── hwpx/         # HWPX(XML) 파서
│       │       └── section.rs # 섹션 파싱 (테이블, 이미지, 중첩 테이블 처리)
│       │
│       ├── document/         # 문서 구조체 정의
│       │   ├── bodytext/     # 본문 (섹션, 문단, 표, 그림)
│       │   ├── docinfo/      # 문서 정보 (폰트, 스타일)
│       │   └── bindata/      # 바이너리 데이터 (이미지)
│       │
│       └── viewer/           # 출력 변환기
│           ├── markdown/     # Markdown 변환
│           │   └── document/bodytext/table.rs  # 테이블 렌더링
│           └── html/         # HTML 변환
│
└── packages/hwpx-python/     # Python 바인딩 (PyO3)
```

## 주요 파일

### 파서 (Parser)
- `crates/hwp-core/src/parser/hwpx/section.rs`: HWPX 섹션 파싱
  - 중첩 테이블 지원 (`TableState` 스택으로 부모 테이블 상태 저장)
  - `table_depth` 추적으로 테이블 중첩 레벨 관리
  - 테이블 셀 내 이미지 파싱

### Markdown 변환
- `crates/hwp-core/src/viewer/markdown/document/bodytext/table.rs`: 테이블 렌더링
  - `convert_table_to_html()`: 테이블을 HTML로 변환
  - `get_cell_content()`: 셀 콘텐츠 추출 (텍스트, 이미지, 중첩 테이블)
  - 중첩 테이블 재귀 렌더링

### Python 바인딩
- `packages/hwpx-python/src/lib.rs`: PyO3 바인딩
  - `parse()`, `parse_file()`: 파싱 함수
  - `Document.to_markdown()`, `to_html()`, `to_json()`, `get_text()`

## 빌드 명령어

```bash
# Rust 빌드
cargo build --release

# Python 휠 빌드 (maturin 사용)
cd packages/hwpx-python
maturin build --release

# 테스트
cargo test

# Clippy 린트
cargo clippy --all-targets --all-features
```

## 개발 참고사항

### 파일 형식
- **HWP 5.0**: Compound File Binary Format (CFB)
- **HWPX**: ZIP 내부 XML 파일들 (OWPML 표준)
  - `section0.xml`: 본문 섹션
  - `BinData/`: 이미지 등 바이너리 데이터

### 테이블 구조 (HWPX)
```xml
<hp:tbl>           <!-- 테이블 -->
  <hp:tr>          <!-- 행 -->
    <hp:tc>        <!-- 셀 -->
      <hp:subList> <!-- 셀 내용 컨테이너 -->
        <hp:p>     <!-- 문단 -->
          <hp:pic> <!-- 이미지 -->
          <hp:tbl> <!-- 중첩 테이블 -->
```

### 주의사항
- 중첩 테이블은 `table_state_stack`으로 부모 상태 저장/복원
- 테이블 셀 내 이미지는 `CellContentItem::Image`로 관리
- Markdown 출력 시 테이블은 HTML `<table>` 태그 사용
