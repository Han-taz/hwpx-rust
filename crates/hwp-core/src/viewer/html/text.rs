/// 텍스트 렌더링 모듈 / Text rendering module
use crate::document::{
    bodytext::{CharShapeInfo, ParaTextRun, Paragraph, ParagraphRecord},
    HwpDocument,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TextSegment<'a> {
    text: &'a str,
    char_shape_id: Option<usize>,
}

fn segment_text_by_char_shapes<'a>(
    text: &'a str,
    char_shapes: &[CharShapeInfo],
) -> Vec<TextSegment<'a>> {
    let byte_offsets: Vec<usize> = text
        .char_indices()
        .map(|(idx, _)| idx)
        .chain(std::iter::once(text.len()))
        .collect();
    let char_count = byte_offsets.len().saturating_sub(1);

    let mut sorted_shapes: Vec<_> = char_shapes.iter().collect();
    sorted_shapes.sort_by_key(|shape| shape.position);

    let mut positions = Vec::with_capacity(sorted_shapes.len() + 2);
    positions.push(0);
    for shape_info in &sorted_shapes {
        let pos = shape_info.position as usize;
        if pos <= char_count {
            positions.push(pos);
        }
    }
    positions.push(char_count);
    positions.sort_unstable();
    positions.dedup();

    let mut segments = Vec::with_capacity(positions.len().saturating_sub(1));
    for bounds in positions.windows(2) {
        let start = bounds[0];
        let end = bounds[1];
        if start >= end {
            continue;
        }

        let shape_idx = sorted_shapes.partition_point(|shape| (shape.position as usize) <= start);
        let char_shape_id = shape_idx
            .checked_sub(1)
            .map(|idx| sorted_shapes[idx].shape_id as usize);

        segments.push(TextSegment {
            text: &text[byte_offsets[start]..byte_offsets[end]],
            char_shape_id,
        });
    }

    segments
}

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

    let segments = segment_text_by_char_shapes(text, char_shapes);

    // 각 구간을 HTML로 렌더링 / Render each segment to HTML
    let mut result = String::new();
    for segment in segments {
        let segment_text = segment.text;
        if segment_text.is_empty() {
            continue;
        }
        let char_shape_id_opt = segment.char_shape_id;

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
        let mut text_for_styling = html_escape(segment_text);
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

                let escaped_url = html_escape_attr(super::security::safe_href_value(url));
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
    super::security::escape_html_text(text)
}

/// HTML 속성 값을 위한 문자 이스케이프 / Escape characters for HTML attribute values
fn html_escape_attr(text: &str) -> String {
    super::security::escape_html_attr(text)
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

/// runs 기반 렌더링이 필요한지 확인 / Check if runs require run-based rendering
pub fn runs_require_run_rendering(runs: &[ParaTextRun]) -> bool {
    runs_have_char_shape_id(runs)
        || runs
            .iter()
            .any(|run| matches!(run, ParaTextRun::Hyperlink { .. }))
}

/// 문단이 runs 기반 렌더링을 우선해야 하는지 확인
/// Check if paragraph should prefer run-based rendering
pub fn paragraph_requires_run_rendering(paragraph: &Paragraph) -> bool {
    let mut runs: &[ParaTextRun] = &[];
    let mut has_para_char_shape = false;

    for record in &paragraph.records {
        match record {
            ParagraphRecord::ParaText { data } => {
                runs = &data.runs;
            }
            ParagraphRecord::ParaCharShape { shapes } => {
                has_para_char_shape = !shapes.is_empty();
            }
            _ => {}
        }
    }

    runs_require_run_rendering(runs) || (!runs.is_empty() && !has_para_char_shape)
}

/// LineSegment 레이아웃에 반영할 렌더링 대상이 있는지 확인
/// Check if paragraph has content that should participate in line-segment layout
pub fn paragraph_has_line_segment_content(paragraph: &Paragraph) -> bool {
    let mut has_text_record = false;
    let mut has_line_segment = false;

    for record in &paragraph.records {
        match record {
            ParagraphRecord::ParaText { data } => {
                has_text_record = true;
                if !data.text.is_empty() || !data.control_char_positions.is_empty() {
                    return true;
                }
            }
            ParagraphRecord::ParaLineSeg { segments } => {
                has_line_segment = has_line_segment || !segments.is_empty();
            }
            ParagraphRecord::ShapeComponent { .. }
            | ParagraphRecord::ShapeComponentPicture { .. }
            | ParagraphRecord::ShapeComponentLine { .. }
            | ParagraphRecord::ShapeComponentRectangle { .. }
            | ParagraphRecord::ShapeComponentEllipse { .. }
            | ParagraphRecord::ShapeComponentArc { .. }
            | ParagraphRecord::ShapeComponentPolygon { .. }
            | ParagraphRecord::ShapeComponentCurve { .. }
            | ParagraphRecord::ShapeComponentOle { .. }
            | ParagraphRecord::ShapeComponentContainer { .. }
            | ParagraphRecord::ShapeComponentTextArt { .. }
            | ParagraphRecord::ShapeComponentUnknown { .. }
            | ParagraphRecord::Table { .. }
            | ParagraphRecord::CtrlHeader { .. } => return true,
            _ => {}
        }
    }

    has_line_segment && !has_text_record
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::bodytext::line_seg::LineSegmentTag;

    fn test_document() -> HwpDocument {
        HwpDocument::new(crate::document::FileHeader {
            signature: "HWP Document File".to_string(),
            version: 0x05000300,
            document_flags: 0,
            license_flags: 0,
            encrypt_version: 0,
            kogl_country: 0,
            reserved: vec![0; 207],
        })
    }

    fn test_line_segment() -> crate::document::bodytext::LineSegmentInfo {
        crate::document::bodytext::LineSegmentInfo {
            text_start_position: 0,
            vertical_position: 0,
            line_height: 100,
            text_height: 100,
            baseline_distance: 85,
            line_spacing: 60,
            column_start_position: 0,
            segment_width: 1000,
            tag: LineSegmentTag {
                is_first_line_of_page: false,
                is_first_line_of_column: false,
                is_empty_segment: false,
                is_first_segment_of_line: true,
                is_last_segment_of_line: true,
                has_auto_hyphenation: false,
                has_indentation: false,
                has_paragraph_header_shape: false,
            },
        }
    }

    #[test]
    fn line_segment_only_paragraph_participates_in_layout() {
        let paragraph = Paragraph {
            para_header: Default::default(),
            records: vec![ParagraphRecord::ParaLineSeg {
                segments: vec![test_line_segment()],
            }],
        };

        assert!(paragraph_has_line_segment_content(&paragraph));
    }

    #[test]
    fn empty_text_with_line_segment_is_layout_metadata_only() {
        let paragraph = Paragraph {
            para_header: Default::default(),
            records: vec![
                ParagraphRecord::ParaText {
                    data: Box::default(),
                },
                ParagraphRecord::ParaLineSeg {
                    segments: vec![test_line_segment()],
                },
            ],
        };

        assert!(!paragraph_has_line_segment_content(&paragraph));
    }

    #[test]
    fn text_segments_use_utf8_boundaries() {
        let shapes = vec![
            CharShapeInfo {
                position: 0,
                shape_id: 1,
            },
            CharShapeInfo {
                position: 2,
                shape_id: 7,
            },
        ];

        let segments = segment_text_by_char_shapes("가a🙂나", &shapes);

        assert_eq!(
            segments,
            vec![
                TextSegment {
                    text: "가a",
                    char_shape_id: Some(1),
                },
                TextSegment {
                    text: "🙂나",
                    char_shape_id: Some(7),
                },
            ]
        );
    }

    #[test]
    fn text_segments_use_last_shape_at_same_position() {
        let shapes = vec![
            CharShapeInfo {
                position: 0,
                shape_id: 1,
            },
            CharShapeInfo {
                position: 0,
                shape_id: 2,
            },
            CharShapeInfo {
                position: 1,
                shape_id: 3,
            },
        ];

        let segments = segment_text_by_char_shapes("abc", &shapes);

        assert_eq!(
            segments,
            vec![
                TextSegment {
                    text: "a",
                    char_shape_id: Some(2),
                },
                TextSegment {
                    text: "bc",
                    char_shape_id: Some(3),
                },
            ]
        );
    }

    #[test]
    fn hyperlink_urls_reject_active_content_schemes() {
        let runs = vec![ParaTextRun::Hyperlink {
            text: "<b>Open</b>".to_string(),
            url: "javascript:alert(1)".to_string(),
            char_shape_id: None,
        }];

        let html = render_text_runs(&runs, &test_document());

        assert!(html.contains("href=\"#\""));
        assert!(!html.contains("href=\"javascript:"));
        assert!(!html.contains("<b>Open</b>"));
        assert!(html.contains("&lt;b&gt;Open&lt;/b&gt;"));
    }
}
