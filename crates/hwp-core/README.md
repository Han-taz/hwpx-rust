# hwp-core

한글과컴퓨터 한/글 문서 파일(.hwp, .hwpx)을 파싱하는 Rust 라이브러리입니다.

## 지원 형식

| 형식 | 설명 | 매직 바이트 |
|------|------|-------------|
| HWP 5.0 | CFB (Compound File Binary) 기반 바이너리 형식 | `D0 CF 11 E0` |
| HWPX | ZIP 기반 XML 형식 (OWPML 표준, KS X 6101) | `PK..` |

## 사용법

### 기본 사용법

```rust
use hwp_core::HwpParser;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = std::fs::read("document.hwp")?;
    let parser = HwpParser::new();

    // HWP/HWPX 자동 감지
    let document = parser.parse(&data)?;

    println!("버전: {}.{}.{}.{}",
        (document.file_header.version >> 24) & 0xFF,
        (document.file_header.version >> 16) & 0xFF,
        (document.file_header.version >> 8) & 0xFF,
        document.file_header.version & 0xFF
    );
    println!("섹션 수: {}", document.body_text.sections.len());

    Ok(())
}
```

### Markdown 변환

```rust
use hwp_core::HwpParser;
use hwp_core::viewer::markdown::{to_markdown, MarkdownOptions};

let document = HwpParser::new().parse(&data)?;

// 기본 옵션
let markdown = to_markdown(&document, &MarkdownOptions::default());

// 커스텀 옵션
let options = MarkdownOptions {
    use_html: Some(true),              // HTML 태그 사용 (테이블)
    include_version: Some(true),       // 버전 정보 포함
    image_output_dir: Some("./images".to_string()), // 이미지 파일 저장
    include_page_info: None,
};
let markdown = to_markdown(&document, &options);
```

### HTML 변환

```rust
use hwp_core::viewer::html::{to_html, HtmlOptions};

let options = HtmlOptions {
    image_output_dir: None,           // None이면 base64 임베딩
    html_output_dir: None,
    include_version: Some(true),
    include_page_info: None,
    css_class_prefix: String::new(),
};
let html = to_html(&document, &options);
```

### JSON 직렬화

```rust
// 전체 문서를 JSON으로 변환
let json = serde_json::to_string_pretty(&document)?;

// FileHeader만 JSON으로 변환
let header_json = parser.parse_fileheader_json(&data)?;
```

## 문서 구조

`HwpDocument` 구조체는 다음과 같은 필드를 포함합니다:

```rust
pub struct HwpDocument {
    pub file_header: FileHeader,           // 파일 헤더 (버전, 플래그)
    pub doc_info: DocInfo,                 // 문서 정보 (폰트, 스타일 등)
    pub body_text: BodyText,               // 본문 (섹션, 문단)
    pub bin_data: BinData,                 // 바이너리 데이터 (이미지)
    pub summary_information: Option<...>,  // 문서 요약
    pub preview_text: Option<...>,         // 미리보기 텍스트
    pub preview_image: Option<...>,        // 미리보기 이미지
    pub scripts: Option<...>,              // 스크립트
    pub xml_template: Option<...>,         // XML 템플릿
}
```

### 주요 모듈

| 모듈 | 설명 |
|------|------|
| `document::fileheader` | 파일 헤더 파싱 (버전, 암호화, 압축 플래그) |
| `document::docinfo` | 문서 정보 (폰트, 문자 모양, 문단 모양, 스타일, 번호 매기기) |
| `document::bodytext` | 본문 파싱 (섹션, 문단, 텍스트, 컨트롤) |
| `document::bindata` | 바이너리 데이터 (이미지, OLE 객체) |
| `viewer::markdown` | Markdown 변환기 |
| `viewer::html` | HTML 변환기 (페이지 레이아웃, SVG 테이블) |
| `cfb` | Compound File Binary 파서 |
| `parser::hwpx` | HWPX (XML) 파서 |

### 지원하는 컨트롤

- **테이블** (`tbl`): 셀 병합, 테두리, 배경색
- **그림** (`pic`): 이미지 삽입, 크기 조절
- **도형**: 사각형, 선, 타원, 다각형, 호, 곡선
- **텍스트 상자** (`txt`): 글상자
- **각주/미주** (`fn`, `en`)
- **머리글/바닥글** (`head`, `foot`)
- **쪽 번호** (`pgnp`)
- **자동 번호** (`atno`)
- **책갈피** (`bok`)
- **하이퍼링크** (`hlnk`)
- **다단** (`cold`)

## 에러 처리

```rust
use hwp_core::{HwpParser, HwpError};

match parser.parse(&data) {
    Ok(doc) => println!("파싱 성공"),
    Err(HwpError::UnknownFormat) => println!("알 수 없는 파일 형식"),
    Err(HwpError::InvalidCfb(e)) => println!("CFB 파싱 오류: {}", e),
    Err(HwpError::StreamNotFound(name)) => println!("스트림 없음: {}", name),
    Err(HwpError::Encrypted) => println!("암호화된 문서"),
    Err(e) => println!("오류: {}", e),
}
```

## 참고 자료

- [한글 문서 파일 형식 5.0](https://www.hancom.com/etc/hwpDownload.do) - 공식 스펙 문서
- [OWPML 표준 (KS X 6101)](https://www.kssn.net/) - HWPX 형식 표준

## 라이선스

MIT
