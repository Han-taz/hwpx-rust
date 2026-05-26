/// 텍스트 렌더링 모듈 / Text rendering module
use crate::document::{
    bodytext::{CharShapeInfo, ParaTextRun, ParagraphRecord},
    HwpDocument,
};

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
            let italic = char_shape.attributes.italic;
            let underline = char_shape.attributes.underline_type > 0;
            let strikethrough = char_shape.attributes.strikethrough > 0;
            let superscript = char_shape.attributes.superscript;
            let subscript = char_shape.attributes.subscript;
            let styled_text = if italic || underline || strikethrough || superscript || subscript {
                let mut buf = String::with_capacity(text_for_styling.len() + 40);
                if italic {
                    buf.push_str("<em>");
                }
                if underline {
                    buf.push_str("<u>");
                }
                if strikethrough {
                    buf.push_str("<s>");
                }
                if superscript {
                    buf.push_str("<sup>");
                }
                if subscript {
                    buf.push_str("<sub>");
                }
                buf.push_str(&text_for_styling);
                if subscript {
                    buf.push_str("</sub>");
                }
                if superscript {
                    buf.push_str("</sup>");
                }
                if strikethrough {
                    buf.push_str("</s>");
                }
                if underline {
                    buf.push_str("</u>");
                }
                if italic {
                    buf.push_str("</em>");
                }
                buf
            } else {
                text_for_styling
            };

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
                    let italic = char_shape.attributes.italic;
                    let underline = char_shape.attributes.underline_type > 0;
                    let strikethrough = char_shape.attributes.strikethrough > 0;
                    let superscript = char_shape.attributes.superscript;
                    let subscript = char_shape.attributes.subscript;
                    let styled_text =
                        if italic || underline || strikethrough || superscript || subscript {
                            let mut buf = String::with_capacity(text_for_styling.len() + 40);
                            if italic {
                                buf.push_str("<em>");
                            }
                            if underline {
                                buf.push_str("<u>");
                            }
                            if strikethrough {
                                buf.push_str("<s>");
                            }
                            if superscript {
                                buf.push_str("<sup>");
                            }
                            if subscript {
                                buf.push_str("<sub>");
                            }
                            buf.push_str(&text_for_styling);
                            if subscript {
                                buf.push_str("</sub>");
                            }
                            if superscript {
                                buf.push_str("</sup>");
                            }
                            if strikethrough {
                                buf.push_str("</s>");
                            }
                            if underline {
                                buf.push_str("</u>");
                            }
                            if italic {
                                buf.push_str("</em>");
                            }
                            buf
                        } else {
                            text_for_styling
                        };

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
            ParagraphRecord::ParaText { data } => {
                text.push_str(&data.text);
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
        if let ParagraphRecord::ParaText { data } = record {
            return data.runs.clone();
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
