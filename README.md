# hwp-rs

한글과컴퓨터의 한/글 문서 파일(.hwp, .hwpx)을 파싱하는 Rust 라이브러리입니다.

본 프로젝트는 한글과컴퓨터의 한/글 문서 파일 형식 공개 문서를 참고하여 개발하였습니다.
[공개 문서 다운로드](https://www.hancom.com/etc/hwpDownload.do)

## 지원 형식

- **HWP 5.0**: 바이너리 형식 (Compound File Binary Format)
- **HWPX**: XML 기반 형식 (OWPML 표준)

## 프로젝트 구조

```
hwp-rs/
├── crates/
│   └── hwp-core/                # 핵심 Rust 라이브러리
│       ├── src/
│       │   ├── lib.rs           # 라이브러리 진입점, HwpParser 정의
│       │   ├── cfb.rs           # Compound File Binary 파서 (HWP 5.0)
│       │   ├── decompress.rs    # zlib 압축 해제
│       │   ├── error.rs         # 에러 타입 정의
│       │   ├── types.rs         # 공통 타입 (HWPUNIT, Color 등)
│       │   │
│       │   ├── parser/          # 파일 형식별 파서
│       │   │   ├── mod.rs       # 파서 통합 (HWP/HWPX 자동 감지)
│       │   │   ├── detect.rs    # 파일 형식 감지
│       │   │   └── hwpx/        # HWPX (XML) 파서
│       │   │
│       │   ├── document/        # 문서 구조체 정의
│       │   │   ├── fileheader/  # 파일 헤더 (버전, 플래그)
│       │   │   ├── docinfo/     # 문서 정보 (폰트, 스타일, 번호매기기)
│       │   │   ├── bodytext/    # 본문 (섹션, 문단, 표, 그림)
│       │   │   │   ├── ctrl_header/      # 컨트롤 헤더 (표, 각주, 머리글 등)
│       │   │   │   └── shape_component/  # 도형 (사각형, 선, 이미지 등)
│       │   │   ├── bindata/     # 바이너리 데이터 (이미지, OLE)
│       │   │   └── scripts/     # 문서 스크립트
│       │   │
│       │   └── viewer/          # 출력 변환기
│       │       ├── markdown/    # Markdown 변환
│       │       ├── html/        # HTML 변환 (페이지 레이아웃, SVG 테이블)
│       │       ├── pdf/         # PDF 변환 (예정)
│       │       └── canvas/      # Canvas 렌더링 (예정)
│       │
│       └── tests/
│           ├── fixtures/        # 테스트용 HWP 파일
│           └── snapshots/       # 스냅샷 테스트 결과
│
└── packages/
    └── hwpx-python/             # Python 바인딩
        ├── src/lib.rs           # PyO3 바인딩 코드
        ├── pyproject.toml       # Python 패키지 설정
        └── Cargo.toml           # Rust 의존성
```

## 기능

- HWP/HWPX 문서 파싱
- Markdown 변환 (테이블, 이미지 지원)
- HTML 변환
- JSON 변환
- 텍스트 추출
- 이미지 추출

## Python 사용법

### 설치

```bash
pip install hwpx
```

### 사용 예제

```python
import hwpx

# 파일에서 문서 열기
doc = hwpx.parse_file("document.hwpx")

# 또는 바이트에서 파싱
with open("document.hwp", "rb") as f:
    doc = hwpx.parse(f.read())

# 문서 정보
print(doc.version)        # 예: "5.1.0.1"
print(doc.section_count)  # 섹션 수

# Markdown 변환
markdown = doc.to_markdown()
markdown = doc.to_markdown(
    use_html=True,              # HTML 태그 사용 (테이블 등)
    include_version=True,       # 버전 정보 포함
    image_output_dir="./images" # 이미지를 파일로 저장 (없으면 base64)
)

# HTML 변환
html = doc.to_html()
html = doc.to_html(image_output_dir="./images")

# JSON 변환
json_str = doc.to_json()

# 텍스트 추출
text = doc.get_text()
```

## Rust 사용법

```rust
use hwp_core::{HwpParser, HwpDocument};
use hwp_core::viewer::markdown::{to_markdown, MarkdownOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = std::fs::read("document.hwp")?;
    let parser = HwpParser::new();
    let doc = parser.parse(&data)?;

    let options = MarkdownOptions::default();
    let markdown = to_markdown(&doc, &options);
    println!("{}", markdown);

    Ok(())
}
```

## 개발

### 빌드

```bash
# Rust 라이브러리 빌드
cargo build --release

# Python 휠 빌드
cd packages/hwpx-python
pip install maturin
maturin build --release
```

### 테스트

```bash
cargo test
```

## 참고 프로젝트

- [pyhwp](https://github.com/mete0r/pyhwp)
- [hwp.js](https://github.com/niceilm/hwp.js)
- [libhwp](https://github.com/niceilm/libhwp)

## 라이선스

MIT
