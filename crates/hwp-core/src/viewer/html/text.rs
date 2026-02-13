/// 텍스트 렌더링 모듈 / Text rendering module
use crate::document::{
    bodytext::{
        control_char::ControlChar, CharShapeInfo, ControlCharPosition, ParaTextRun, ParagraphRecord,
    },
    HwpDocument,
};

/// 하이퍼링크 영역 정보 (HTML용) / Hyperlink region information (for HTML)
/// NOTE: 현재 HTML에서 하이퍼링크 처리는 세그먼트 단위 위치 변환이 복잡하여 미사용
/// Currently unused as HTML hyperlink processing is complex due to segment-level position conversion
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct HtmlHyperlinkRegion {
    /// URL
    pub url: String,
}

/// RESERVED_3 (code 3) - 필드 콘텐츠 시작 / Field content start
#[allow(dead_code)]
const FIELD_CONTENT_START: u8 = 3;

/// 텍스트를 HTML로 렌더링 / Render text to HTML
pub fn render_text(
    text: &str,
    char_shapes: &[CharShapeInfo],
    document: &HwpDocument,
    _css_prefix: &str,
) -> String {
    if text.is_empty() {
        return String::new();
    }

    let text_chars: Vec<char> = text.chars().collect();
    let text_len = text_chars.len();

    // CharShape 구간 계산 / Calculate CharShape segments
    let mut segments: Vec<(usize, usize, Option<usize>)> = Vec::new();

    // CharShape 정보를 position 기준으로 정렬 / Sort CharShape info by position
    let mut sorted_shapes: Vec<_> = char_shapes.iter().collect();
    sorted_shapes.sort_by_key(|shape| shape.position);

    // 구간 정의 / Define segments
    let mut positions = vec![0];
    for shape_info in &sorted_shapes {
        let pos = shape_info.position as usize;
        if pos <= text_len {
            positions.push(pos);
        }
    }
    positions.push(text_len);
    positions.sort();
    positions.dedup();

    // 각 구간에 대한 CharShape 찾기 / Find CharShape for each segment
    for i in 0..positions.len() - 1 {
        let start = positions[i];
        let end = positions[i + 1];

        // 이 구간에 해당하는 CharShape 찾기 / Find CharShape for this segment
        let char_shape_id = sorted_shapes
            .iter()
            .rev()
            .find(|shape| (shape.position as usize) <= start)
            .map(|shape| shape.shape_id as usize);

        segments.push((start, end, char_shape_id));
    }

    // 각 구간을 HTML로 렌더링 / Render each segment to HTML
    let mut result = String::new();
    for (start, end, char_shape_id_opt) in segments {
        if start >= end {
            continue;
        }

        let segment_text: String = text_chars[start..end].iter().collect();
        if segment_text.is_empty() {
            continue;
        }

        // CharShape 가져오기 / Get CharShape
        // HWP 파일의 shape_id는 0-based indexing을 사용합니다 / HWP file uses 0-based indexing for shape_id
        let char_shape_opt = char_shape_id_opt.and_then(|id| {
            if id < document.doc_info.char_shapes.len() {
                document.doc_info.char_shapes.get(id)
            } else {
                None
            }
        });

        // 텍스트 스타일 적용 / Apply text styles
        // HTML 특수 문자 이스케이프 (XSS 방지) / Escape HTML special characters (prevent XSS)
        let mut text_for_styling = html_escape(&segment_text);
        // 첫 공백을 &nbsp;로 변환 / Convert leading space to &nbsp;
        if text_for_styling.starts_with(' ') {
            text_for_styling = text_for_styling.replacen(' ', "&nbsp;", 1);
        }
        // 마지막 공백을 &nbsp;로 변환 / Convert trailing space to &nbsp;
        if text_for_styling.ends_with(' ') {
            text_for_styling.pop();
            text_for_styling.push_str("&nbsp;");
        }

        if let Some(char_shape) = char_shape_opt {
            // CharShape 클래스 적용 / Apply CharShape class (0-based indexing to match XSL/XML format)
            let class_name = format!("cs{}", char_shape_id_opt.unwrap_or(0));

            // 인라인 스타일 추가 / Add inline styles
            let mut inline_style = String::new();

            // 폰트 크기 / Font size
            let size_pt = char_shape.base_size as f64 / 100.0;
            inline_style.push_str(&format!("font-size:{size_pt}pt;"));

            // 텍스트 색상 / Text color
            let color = &char_shape.text_color;
            inline_style.push_str(&format!(
                "color:rgb({},{},{});",
                color.r(),
                color.g(),
                color.b()
            ));

            // 속성 / Attributes
            // bold는 CSS의 font-weight:bold로 처리되므로 <strong> 태그 사용하지 않음
            // Bold is handled by CSS font-weight:bold, so don't use <strong> tag
            let mut styled_text = text_for_styling;
            if char_shape.attributes.italic {
                styled_text = format!("<em>{styled_text}</em>");
            }
            if char_shape.attributes.underline_type > 0 {
                styled_text = format!("<u>{styled_text}</u>");
            }
            if char_shape.attributes.strikethrough > 0 {
                styled_text = format!("<s>{styled_text}</s>");
            }
            if char_shape.attributes.superscript {
                styled_text = format!("<sup>{styled_text}</sup>");
            }
            if char_shape.attributes.subscript {
                styled_text = format!("<sub>{styled_text}</sub>");
            }

            // .hrt span으로 래핑 / Wrap with .hrt span
            if !inline_style.is_empty() {
                result.push_str(&format!(
                    r#"<span class="hrt {class_name}" style="{inline_style}">{styled_text}</span>"#
                ));
            } else {
                result.push_str(&format!(
                    r#"<span class="hrt {class_name}">{styled_text}</span>"#
                ));
            }
        } else {
            // CharShape가 없는 경우 기본 스타일 / Default style when no CharShape
            result.push_str(&format!(r#"<span class="hrt">{text_for_styling}</span>"#));
        }
    }

    result
}

/// 텍스트 run들을 HTML로 렌더링 (HWPX charPrIDRef 지원)
/// Render text runs to HTML (HWPX charPrIDRef support)
pub fn render_text_runs(runs: &[ParaTextRun], document: &HwpDocument) -> String {
    let mut result = String::new();

    for run in runs {
        match run {
            ParaTextRun::Text {
                text,
                char_shape_id,
            } => {
                if text.is_empty() {
                    continue;
                }

                // CharShape 가져오기 / Get CharShape
                let char_shape_opt =
                    char_shape_id.and_then(|id| document.doc_info.char_shapes.get(id as usize));

                // 텍스트 스타일 적용 / Apply text styles
                let mut text_for_styling = html_escape(text);

                // 첫 공백과 마지막 공백을 &nbsp;로 변환
                if text_for_styling.starts_with(' ') {
                    text_for_styling = text_for_styling.replacen(' ', "&nbsp;", 1);
                }
                if text_for_styling.ends_with(' ') {
                    text_for_styling.pop();
                    text_for_styling.push_str("&nbsp;");
                }

                if let Some(char_shape) = char_shape_opt {
                    let class_name = format!("cs{}", char_shape_id.unwrap_or(0));

                    // 인라인 스타일 생성 / Generate inline styles
                    let mut inline_style = String::new();

                    // 폰트 크기 / Font size
                    let size_pt = char_shape.base_size as f64 / 100.0;
                    inline_style.push_str(&format!("font-size:{size_pt}pt;"));

                    // 텍스트 색상 / Text color
                    let color = &char_shape.text_color;
                    if color.0 != 0 {
                        inline_style.push_str(&format!(
                            "color:rgb({},{},{});",
                            color.r(),
                            color.g(),
                            color.b()
                        ));
                    }

                    // bold
                    if char_shape.attributes.bold {
                        inline_style.push_str("font-weight:bold;");
                    }

                    // 스타일 태그 적용 / Apply style tags
                    let mut styled_text = text_for_styling;
                    if char_shape.attributes.italic {
                        styled_text = format!("<em>{styled_text}</em>");
                    }
                    if char_shape.attributes.underline_type > 0 {
                        styled_text = format!("<u>{styled_text}</u>");
                    }
                    if char_shape.attributes.strikethrough > 0 {
                        styled_text = format!("<s>{styled_text}</s>");
                    }
                    if char_shape.attributes.superscript {
                        styled_text = format!("<sup>{styled_text}</sup>");
                    }
                    if char_shape.attributes.subscript {
                        styled_text = format!("<sub>{styled_text}</sub>");
                    }

                    // span으로 래핑 / Wrap with span
                    if !inline_style.is_empty() {
                        result.push_str(&format!(
                            r#"<span class="hrt {class_name}" style="{inline_style}">{styled_text}</span>"#
                        ));
                    } else {
                        result.push_str(&format!(
                            r#"<span class="hrt {class_name}">{styled_text}</span>"#
                        ));
                    }
                } else {
                    // CharShape가 없는 경우 기본 스타일 / Default style when no CharShape
                    result.push_str(&format!(r#"<span class="hrt">{text_for_styling}</span>"#));
                }
            }
            ParaTextRun::Control { display_text, .. } => {
                // 컨트롤 문자의 display_text 렌더링 / Render control character display_text
                if let Some(text) = display_text {
                    result.push_str(&html_escape(text));
                }
            }
            ParaTextRun::Hyperlink {
                text,
                url,
                char_shape_id,
            } => {
                // 하이퍼링크 렌더링 / Render hyperlink
                if text.is_empty() {
                    continue;
                }

                let escaped_url = html_escape_attr(url);
                let mut link_text = html_escape(text);

                // CharShape 스타일 적용 / Apply CharShape styles
                let char_shape_opt =
                    char_shape_id.and_then(|id| document.doc_info.char_shapes.get(id as usize));

                let mut inline_style = String::new();
                if let Some(char_shape) = char_shape_opt {
                    let size_pt = char_shape.base_size as f64 / 100.0;
                    inline_style.push_str(&format!("font-size:{size_pt}pt;"));

                    let color = &char_shape.text_color;
                    if color.0 != 0 {
                        inline_style.push_str(&format!(
                            "color:rgb({},{},{});",
                            color.r(),
                            color.g(),
                            color.b()
                        ));
                    }

                    if char_shape.attributes.bold {
                        link_text = format!("<strong>{link_text}</strong>");
                    }
                    if char_shape.attributes.italic {
                        link_text = format!("<em>{link_text}</em>");
                    }
                }

                if !inline_style.is_empty() {
                    result.push_str(&format!(
                        r#"<a href="{escaped_url}" class="hwp-link" style="{inline_style}">{link_text}</a>"#
                    ));
                } else {
                    result.push_str(&format!(
                        r#"<a href="{escaped_url}" class="hwp-link">{link_text}</a>"#
                    ));
                }
            }
        }
    }

    result
}

/// HTML 특수 문자 이스케이프 / Escape HTML special characters
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// HTML 속성 값을 위한 문자 이스케이프 / Escape characters for HTML attribute values
fn html_escape_attr(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 문단에서 텍스트와 CharShape 추출 / Extract text and CharShape from paragraph
pub fn extract_text_and_shapes(
    paragraph: &crate::document::bodytext::Paragraph,
) -> (String, Vec<CharShapeInfo>) {
    let mut text = String::new();
    let mut char_shapes = Vec::new();

    for record in &paragraph.records {
        match record {
            ParagraphRecord::ParaText {
                text: para_text, ..
            } => {
                text.push_str(para_text);
            }
            ParagraphRecord::ParaCharShape { shapes } => {
                char_shapes.extend(shapes.iter().cloned());
            }
            _ => {}
        }
    }

    (text, char_shapes)
}

/// 문단에서 텍스트 runs 추출 / Extract text runs from paragraph
pub fn extract_runs(paragraph: &crate::document::bodytext::Paragraph) -> Vec<ParaTextRun> {
    for record in &paragraph.records {
        if let ParagraphRecord::ParaText { runs, .. } = record {
            return runs.clone();
        }
    }
    Vec::new()
}

/// runs에 char_shape_id가 있는지 확인 / Check if runs have char_shape_id
pub fn runs_have_char_shape_id(runs: &[ParaTextRun]) -> bool {
    runs.iter().any(|run| {
        matches!(
            run,
            ParaTextRun::Text {
                char_shape_id: Some(_),
                ..
            }
        )
    })
}

/// 제어 문자 크기 (WCHAR 단위) / Control character size (in WCHAR units)
#[allow(dead_code)]
fn get_control_char_size(code: u8) -> usize {
    // CHAR 타입: NULL(0), LINE_BREAK(10), PARA_BREAK(13), HYPHEN(24), BOUND_SPACE(30), FIXED_SPACE(31)
    if matches!(code, 0 | 10 | 13 | 24 | 30 | 31) {
        1
    } else {
        8
    }
}

/// 원본 스트림 위치를 실제 텍스트 위치로 변환
/// Convert original stream position to actual text position
#[allow(dead_code)]
fn convert_stream_position_to_text_position(
    stream_position: usize,
    control_positions: &[ControlCharPosition],
) -> usize {
    let mut offset = 0;
    for pos in control_positions {
        if pos.position < stream_position {
            offset += get_control_char_size(pos.code);
        }
    }
    stream_position.saturating_sub(offset)
}

/// HTML 특수 문자 이스케이프 / Escape HTML special characters
#[allow(dead_code)]
fn escape_html_attr(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 하이퍼링크 정보를 사용하여 텍스트를 HTML로 렌더링
/// Render text to HTML with hyperlink information
///
/// NOTE: 현재 HTML에서 하이퍼링크 처리는 세그먼트 단위 위치 변환이 복잡하여 미사용
/// Currently unused as HTML hyperlink processing is complex due to segment-level position conversion
///
/// # Arguments
/// * `text` - 텍스트 내용 / Text content
/// * `char_shapes` - 글자 모양 정보 / Character shape information
/// * `control_positions` - 제어 문자 위치 정보 / Control character positions
/// * `hyperlinks` - 하이퍼링크 정보 리스트 / Hyperlink information list
/// * `document` - HWP 문서 / HWP document
/// * `css_prefix` - CSS 클래스 접두사 / CSS class prefix
///
/// # Returns
/// HTML 문자열 / HTML string
#[allow(dead_code)]
pub fn render_text_with_hyperlinks(
    text: &str,
    char_shapes: &[CharShapeInfo],
    control_positions: &[ControlCharPosition],
    hyperlinks: &[HtmlHyperlinkRegion],
    document: &HwpDocument,
    css_prefix: &str,
) -> String {
    if text.is_empty() || hyperlinks.is_empty() {
        return render_text(text, char_shapes, document, css_prefix);
    }

    // 필드 영역 찾기 / Find field regions
    let mut field_regions: Vec<(usize, usize)> = Vec::new();
    let mut sorted_positions: Vec<_> = control_positions.iter().collect();
    sorted_positions.sort_by_key(|p| p.position);

    let mut current_field_start: Option<usize> = None;
    for pos in &sorted_positions {
        if pos.code == FIELD_CONTENT_START {
            current_field_start = Some(pos.position + get_control_char_size(pos.code));
        } else if pos.code == ControlChar::FIELD_END {
            if let Some(start) = current_field_start.take() {
                field_regions.push((start, pos.position));
            }
        }
    }

    // 필드 영역 수와 하이퍼링크 수가 다르면 기본 렌더링 사용
    if field_regions.len() != hyperlinks.len() {
        return render_text(text, char_shapes, document, css_prefix);
    }

    let text_chars: Vec<char> = text.chars().collect();
    let text_len = text_chars.len();

    let mut result = String::new();
    let mut current_text_pos = 0;

    // 각 필드 영역 처리 / Process each field region
    for (i, (stream_start, stream_end)) in field_regions.iter().enumerate() {
        let text_start = convert_stream_position_to_text_position(*stream_start, control_positions);
        let text_end = convert_stream_position_to_text_position(*stream_end, control_positions);

        // 필드 이전 텍스트 렌더링 / Render text before field
        if current_text_pos < text_start && text_start <= text_len {
            let before_text: String = text_chars[current_text_pos..text_start].iter().collect();
            if !before_text.is_empty() {
                // CharShape 구간 필터링 / Filter CharShape segments
                let filtered_shapes: Vec<CharShapeInfo> = char_shapes
                    .iter()
                    .filter(|s| (s.position as usize) < text_start)
                    .cloned()
                    .collect();
                result.push_str(&render_text(
                    &before_text,
                    &filtered_shapes,
                    document,
                    css_prefix,
                ));
            }
        }

        // 필드 내부 텍스트 (하이퍼링크) / Field content (hyperlink)
        if text_start < text_end && text_end <= text_len {
            let link_text: String = text_chars[text_start..text_end].iter().collect();
            if !link_text.trim().is_empty() {
                if let Some(hyperlink) = hyperlinks.get(i) {
                    let escaped_url = escape_html_attr(&hyperlink.url);
                    // CharShape 구간 필터링 / Filter CharShape segments
                    let filtered_shapes: Vec<CharShapeInfo> = char_shapes
                        .iter()
                        .filter(|s| {
                            let pos = s.position as usize;
                            pos >= text_start && pos < text_end
                        })
                        .cloned()
                        .collect();
                    let inner_html =
                        render_text(&link_text, &filtered_shapes, document, css_prefix);
                    result.push_str(&format!(
                        r#"<a href="{}" class="hwp-link">{}</a>"#,
                        escaped_url, inner_html
                    ));
                } else {
                    let filtered_shapes: Vec<CharShapeInfo> = char_shapes
                        .iter()
                        .filter(|s| {
                            let pos = s.position as usize;
                            pos >= text_start && pos < text_end
                        })
                        .cloned()
                        .collect();
                    result.push_str(&render_text(
                        &link_text,
                        &filtered_shapes,
                        document,
                        css_prefix,
                    ));
                }
            }
        }

        current_text_pos = text_end;
    }

    // 마지막 필드 이후 텍스트 렌더링 / Render text after last field
    if current_text_pos < text_len {
        let after_text: String = text_chars[current_text_pos..].iter().collect();
        if !after_text.is_empty() {
            let filtered_shapes: Vec<CharShapeInfo> = char_shapes
                .iter()
                .filter(|s| (s.position as usize) >= current_text_pos)
                .cloned()
                .collect();
            result.push_str(&render_text(
                &after_text,
                &filtered_shapes,
                document,
                css_prefix,
            ));
        }
    }

    result
}
