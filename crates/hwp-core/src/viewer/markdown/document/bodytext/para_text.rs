/// ParaText conversion to Markdown
/// ParaText를 마크다운으로 변환하는 모듈
///
/// 스펙 문서 매핑: 표 57 - 본문의 데이터 레코드, PARA_TEXT (HWPTAG_BEGIN + 51)
/// Spec mapping: Table 57 - BodyText data records, PARA_TEXT (HWPTAG_BEGIN + 51)
use crate::document::bodytext::{CharShapeInfo, ControlChar, ControlCharPosition, ParaTextRun};
use crate::document::{CharShape, HwpDocument};

/// 하이퍼링크 영역 정보 / Hyperlink region information
#[derive(Debug, Clone)]
pub struct HyperlinkRegion {
    /// URL
    pub url: String,
}

/// RESERVED_3 (code 3) - 필드 콘텐츠 시작 / Field content start
const FIELD_CONTENT_START: u8 = 3;

/// 의미 있는 텍스트인지 확인합니다. / Check if text is meaningful.
///
/// 공백만 있는 텍스트는 의미 없다고 판단합니다.
/// Text containing only whitespace is considered meaningless.
///
/// # Arguments / 매개변수
/// * `text` - 제어 문자가 이미 제거된 텍스트 / Text with control characters already removed
/// * `control_positions` - 제어 문자 위치 정보 (현재는 사용되지 않음) / Control character positions (currently unused)
///
/// # Returns / 반환값
/// 의미 있는 텍스트이면 `true`, 그렇지 않으면 `false` / `true` if meaningful, `false` otherwise
///
/// # Note
/// 제어 문자는 이미 파싱 단계에서 text에서 제거되었으므로,
/// 텍스트가 비어있지 않은지만 확인합니다.
/// Control characters are already removed from text during parsing,
/// so we only check if text is not empty.
pub(crate) fn is_meaningful_text(text: &str, _control_positions: &[ControlCharPosition]) -> bool {
    !text.trim().is_empty()
}

/// Convert ParaText to markdown
/// ParaText를 마크다운으로 변환
///
/// # Arguments / 매개변수
/// * `text` - 텍스트 내용 / Text content
/// * `control_positions` - 제어 문자 위치 정보 / Control character positions
///
/// # Returns / 반환값
/// 마크다운 문자열 / Markdown string
/// Convert character index to byte index in UTF-8 string
/// UTF-8 문자열에서 문자 인덱스를 바이트 인덱스로 변환
fn char_index_to_byte_index(text: &str, char_idx: usize) -> Option<usize> {
    text.char_indices()
        .nth(char_idx)
        .map(|(byte_idx, _)| byte_idx)
}

/// CharShape 정보를 사용하여 텍스트를 구간별로 나누고 마크다운 스타일을 적용
/// Divide text into segments by CharShape information and apply markdown styles
fn convert_text_with_char_shapes<'a>(
    text: &str,
    control_positions: &[ControlCharPosition],
    char_shapes: &[CharShapeInfo],
    get_char_shape: &'a dyn Fn(u32) -> Option<&'a CharShape>,
) -> Option<String> {
    if text.trim().is_empty() {
        return None;
    }

    let byte_offsets: Vec<usize> = text.char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let char_count = byte_offsets.len() - 1;

    // CharShape 정보를 position 기준으로 정렬 / Sort CharShape info by position
    let mut sorted_shapes: Vec<_> = char_shapes.iter().collect();
    sorted_shapes.sort_by_key(|shape| shape.position);

    let mut result = String::new();
    let mut segments: Vec<(usize, usize, Option<&CharShape>)> = Vec::new();

    // 구간 정의 / Define segments
    let mut positions = vec![0];
    for shape_info in &sorted_shapes {
        let pos = shape_info.position as usize;
        if pos <= char_count {
            positions.push(pos);
        }
    }
    positions.push(char_count);
    positions.sort();
    positions.dedup();

    // 각 구간에 대한 CharShape 찾기 / Find CharShape for each segment
    for i in 0..positions.len() - 1 {
        let start = positions[i];
        let end = positions[i + 1];

        // 이 구간에 적용할 CharShape 찾기 (이진 탐색) / Find CharShape to apply to this segment (binary search)
        // partition_point returns the index where start would be inserted (position > start)
        // The shape at idx - 1 is the last one with position <= start
        let char_shape = {
            let idx = sorted_shapes.partition_point(|shape| (shape.position as usize) <= start);
            if idx > 0 {
                Some(&sorted_shapes[idx - 1])
            } else {
                None
            }
        }
        .and_then(|shape| get_char_shape(shape.shape_id));

        segments.push((start, end, char_shape));
    }

    // PARA_BREAK/LINE_BREAK 위치 수집 / Collect PARA_BREAK/LINE_BREAK positions
    let mut break_positions: Vec<usize> = control_positions
        .iter()
        .filter(|pos| pos.code == ControlChar::PARA_BREAK || pos.code == ControlChar::LINE_BREAK)
        .map(|pos| pos.position)
        .collect();
    break_positions.sort();

    // 각 구간에 스타일 적용하여 결과 생성 / Generate result by applying styles to each segment
    for (start, end, char_shape) in &segments {
        if *start < *end && *end <= char_count {
            // 이 구간 내에 PARA_BREAK/LINE_BREAK가 있는지 확인 / Check if there are breaks in this segment
            let mut segment_breaks: Vec<usize> = break_positions
                .iter()
                .filter(|&&break_pos| break_pos >= *start && break_pos < *end)
                .copied()
                .collect();

            if segment_breaks.is_empty() {
                // 구간 내에 break가 없으면 전체 구간에 스타일 적용 / No breaks in segment, apply style to entire segment
                let segment_text = &text[byte_offsets[*start]..byte_offsets[*end]];
                if !segment_text.trim().is_empty() {
                    if let Some(shape) = char_shape {
                        let styled = apply_markdown_styles(
                            segment_text,
                            shape.attributes.bold,
                            shape.attributes.italic,
                            shape.attributes.strikethrough != 0,
                            shape.attributes.underline_type != 0,
                        );
                        result.push_str(&styled);
                    } else {
                        result.push_str(segment_text);
                    }
                }
            } else {
                // 구간 내에 break가 있으면 break 위치로 나누어 처리 / Split segment by breaks
                segment_breaks.insert(0, *start);
                segment_breaks.push(*end);

                for i in 0..segment_breaks.len() - 1 {
                    let seg_start = segment_breaks[i];
                    let seg_end = segment_breaks[i + 1];

                    if seg_start < seg_end && seg_end <= char_count {
                        let segment_text = &text[byte_offsets[seg_start]..byte_offsets[seg_end]];
                        if !segment_text.trim().is_empty() {
                            if let Some(shape) = char_shape {
                                let styled = apply_markdown_styles(
                                    segment_text,
                                    shape.attributes.bold,
                                    shape.attributes.italic,
                                    shape.attributes.strikethrough != 0,
                                    shape.attributes.underline_type != 0,
                                );
                                result.push_str(&styled);
                            } else {
                                result.push_str(segment_text);
                            }
                        }
                    }

                    // break 다음이면 마크다운 개행 추가 / Add markdown line break after break
                    if i < segment_breaks.len() - 2 {
                        result.push_str("  \n");
                    }
                }
            }
        }
    }

    let trimmed_result = result.trim();
    if !trimmed_result.is_empty() {
        Some(trimmed_result.to_string())
    } else {
        None
    }
}

/// 텍스트에 마크다운 스타일을 적용합니다. / Apply markdown styles to text.
///
/// # Arguments / 매개변수
/// * `text` - 원본 텍스트 / Original text
/// * `bold` - 진하게 여부 / Bold
/// * `italic` - 기울임 여부 / Italic
/// * `strikethrough` - 가운뎃줄 여부 / Strikethrough
///
/// # Returns / 반환값
/// 마크다운 스타일이 적용된 텍스트 / Text with markdown styles applied
fn apply_markdown_styles(
    text: &str,
    bold: bool,
    italic: bool,
    strikethrough: bool,
    underline: bool,
) -> String {
    if text.is_empty() {
        return String::new();
    }

    let mut result = String::from(text);

    // 마크다운 스타일 적용 순서: underline (가장 바깥) -> strikethrough -> bold -> italic (가장 안쪽)
    // Markdown style application order: underline (outermost) -> strikethrough -> bold -> italic (innermost)

    // 기울임 적용 (가장 안쪽) / Apply italic (innermost)
    if italic {
        result = format!("*{result}*");
    }

    // 진하게 적용 / Apply bold
    if bold {
        result = format!("**{result}**");
    }

    // 가운뎃줄 적용 / Apply strikethrough
    if strikethrough {
        result = format!("~~{result}~~");
    }

    // 밑줄 적용 (가장 바깥쪽, HTML 태그) / Apply underline (outermost, HTML tag)
    if underline {
        result = format!("<u>{result}</u>");
    }

    result
}

/// HWPX runs를 마크다운으로 변환 (char_shape_id 사용)
/// Convert HWPX runs to markdown (using char_shape_id)
pub fn convert_runs_to_markdown(runs: &[ParaTextRun], document: &HwpDocument) -> Option<String> {
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

                let trimmed = text.trim();
                if trimmed.is_empty() {
                    // 공백만 있으면 공백 유지
                    if !text.is_empty() {
                        result.push(' ');
                    }
                    continue;
                }

                // CharShape 가져오기
                let char_shape =
                    char_shape_id.and_then(|id| document.doc_info.char_shapes.get(id as usize));

                if let Some(shape) = char_shape {
                    let styled = apply_markdown_styles(
                        trimmed,
                        shape.attributes.bold,
                        shape.attributes.italic,
                        shape.attributes.strikethrough != 0,
                        shape.attributes.underline_type != 0,
                    );
                    result.push_str(&styled);
                } else {
                    result.push_str(trimmed);
                }
            }
            ParaTextRun::Control { display_text, .. } => {
                if let Some(text) = display_text {
                    result.push_str(text);
                }
            }
            ParaTextRun::Hyperlink {
                text,
                url,
                char_shape_id,
            } => {
                if text.is_empty() {
                    continue;
                }

                // CharShape 스타일 적용 / Apply CharShape styles
                let char_shape =
                    char_shape_id.and_then(|id| document.doc_info.char_shapes.get(id as usize));

                let styled_text = if let Some(shape) = char_shape {
                    apply_markdown_styles(
                        text.trim(),
                        shape.attributes.bold,
                        shape.attributes.italic,
                        shape.attributes.strikethrough != 0,
                        shape.attributes.underline_type != 0,
                    )
                } else {
                    text.trim().to_string()
                };

                // 마크다운 하이퍼링크 형식 / Markdown hyperlink format
                result.push_str(&format!("[{}]({})", styled_text, url));
            }
        }
    }

    let trimmed_result = result.trim();
    if !trimmed_result.is_empty() {
        Some(trimmed_result.to_string())
    } else {
        None
    }
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

pub fn convert_para_text_to_markdown(
    text: &str,
    control_positions: &[ControlCharPosition],
) -> Option<String> {
    convert_para_text_to_markdown_with_char_shapes(text, control_positions, &[], None)
}

/// CharShape 정보를 사용하여 ParaText를 마크다운으로 변환
/// Convert ParaText to markdown using CharShape information
///
/// # Arguments / 매개변수
/// * `text` - 텍스트 내용 / Text content
/// * `control_positions` - 제어 문자 위치 정보 / Control character positions
/// * `char_shapes` - 글자 모양 정보 리스트 / Character shape information list
/// * `get_char_shape` - shape_id로 CharShape를 가져오는 함수 / Function to get CharShape by shape_id
///
/// # Returns / 반환값
/// 마크다운 문자열 / Markdown string
pub fn convert_para_text_to_markdown_with_char_shapes<'a>(
    text: &str,
    control_positions: &[ControlCharPosition],
    char_shapes: &[CharShapeInfo],
    get_char_shape: Option<&'a dyn Fn(u32) -> Option<&'a CharShape>>,
) -> Option<String> {
    // CharShape 정보가 있으면 텍스트를 구간별로 나누어 스타일 적용 / If CharShape info exists, divide text into segments and apply styles
    if !char_shapes.is_empty() {
        if let Some(char_shape_fn) = get_char_shape {
            return convert_text_with_char_shapes(
                text,
                control_positions,
                char_shapes,
                char_shape_fn,
            );
        }
    }

    // PARA_BREAK나 LINE_BREAK가 있는지 확인 / Check for PARA_BREAK or LINE_BREAK
    let has_breaks = control_positions
        .iter()
        .any(|pos| pos.code == ControlChar::PARA_BREAK || pos.code == ControlChar::LINE_BREAK);

    if !has_breaks {
        // 제어 문자가 없으면 기존 로직 사용 / Use existing logic if no control characters
        if is_meaningful_text(text, control_positions) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        return None;
    }

    // PARA_BREAK/LINE_BREAK가 있는 경우 처리 / Process when PARA_BREAK/LINE_BREAK exists
    // 파서에서 \n을 제거했으므로, control_positions의 정보만 사용하여 마크다운 개행으로 변환
    // Parser removed \n, so only use control_positions info to convert to markdown line breaks
    let mut result = String::new();
    let mut last_char_pos = 0;

    // control_positions를 정렬하여 순서대로 처리 / Sort control_positions to process in order
    let mut sorted_positions: Vec<_> = control_positions.iter().collect();
    sorted_positions.sort_by_key(|pos| pos.position);

    for pos in sorted_positions {
        // PARA_BREAK나 LINE_BREAK만 처리 / Only process PARA_BREAK or LINE_BREAK
        if pos.code != ControlChar::PARA_BREAK && pos.code != ControlChar::LINE_BREAK {
            continue;
        }

        // 문자 인덱스를 바이트 인덱스로 변환 / Convert character index to byte index
        let byte_idx = match char_index_to_byte_index(text, pos.position) {
            Some(idx) => idx,
            None => continue, // 유효하지 않은 위치는 건너뜀 / Skip invalid position
        };

        let last_byte_idx = char_index_to_byte_index(text, last_char_pos).unwrap_or(0);

        // 제어 문자 이전의 텍스트 추가 (파서에서 \n이 제거되었으므로 그대로 사용)
        // Add text before control character (parser removed \n, so use as-is)
        if byte_idx > last_byte_idx && byte_idx <= text.len() {
            let text_part = &text[last_byte_idx..byte_idx];
            let trimmed = text_part.trim();
            if !trimmed.is_empty() {
                result.push_str(trimmed);
            }
        }

        // PARA_BREAK나 LINE_BREAK를 마크다운 개행(스페이스 2개 + 개행)으로 변환
        // Convert PARA_BREAK or LINE_BREAK to markdown line break (two spaces + newline)
        result.push_str("  \n");

        // 제어 문자 다음 위치 / Position after control character
        last_char_pos = pos.position + 1;
    }

    // 마지막 부분의 텍스트 추가 / Add remaining text
    let last_byte_idx = char_index_to_byte_index(text, last_char_pos).unwrap_or(0);
    if last_byte_idx < text.len() {
        let text_part = &text[last_byte_idx..];
        let trimmed = text_part.trim();
        if !trimmed.is_empty() {
            result.push_str(trimmed);
        }
    }

    // trim() 대신 trim_start()만 사용하여 줄 끝 공백(마크다운 개행용)은 유지
    // Use trim_start() instead of trim() to preserve trailing spaces (for markdown line breaks)
    let trimmed_result = result.trim_start();
    if !trimmed_result.is_empty() {
        Some(trimmed_result.to_string())
    } else {
        None
    }
}

/// 제어 문자 크기 (WCHAR 단위) / Control character size (in WCHAR units)
/// CHAR 타입: 1 WCHAR, INLINE/EXTENDED 타입: 8 WCHAR
fn get_control_char_size(code: u8) -> usize {
    // CHAR 타입: NULL(0), LINE_BREAK(10), PARA_BREAK(13), HYPHEN(24), BOUND_SPACE(30), FIXED_SPACE(31)
    if matches!(code, 0 | 10 | 13 | 24 | 30 | 31) {
        1
    } else {
        // INLINE 및 EXTENDED 타입: 8 WCHAR
        8
    }
}

/// 원본 스트림 위치를 실제 텍스트 위치로 변환
/// Convert original stream position to actual text position
///
/// 제어 문자는 파서에서 제거되므로, control_positions의 position을 실제 텍스트 인덱스로 변환
/// Control characters are removed by parser, so convert control_positions position to actual text index
fn convert_stream_position_to_text_position(
    stream_position: usize,
    control_positions: &[ControlCharPosition],
) -> usize {
    // 해당 position 이전의 모든 제어 문자 크기 합을 계산
    // Calculate sum of all control character sizes before this position
    let mut offset = 0;
    for pos in control_positions {
        if pos.position < stream_position {
            offset += get_control_char_size(pos.code);
        }
    }
    // 실제 텍스트 위치 = 원본 position - 제어 문자 오프셋
    stream_position.saturating_sub(offset)
}

/// 하이퍼링크 정보를 사용하여 ParaText를 마크다운으로 변환
/// Convert ParaText to markdown using hyperlink information
///
/// # Arguments / 매개변수
/// * `text` - 텍스트 내용 / Text content
/// * `control_positions` - 제어 문자 위치 정보 / Control character positions
/// * `hyperlinks` - 하이퍼링크 정보 리스트 (순서대로) / Hyperlink information list (in order)
///
/// # Returns / 반환값
/// 마크다운 문자열 / Markdown string
pub fn convert_para_text_to_markdown_with_hyperlinks(
    text: &str,
    control_positions: &[ControlCharPosition],
    hyperlinks: &[HyperlinkRegion],
) -> Option<String> {
    if text.trim().is_empty() || hyperlinks.is_empty() {
        // 하이퍼링크가 없으면 기본 변환 사용 / Use default conversion if no hyperlinks
        return convert_para_text_to_markdown(text, control_positions);
    }

    // 필드 영역 찾기 (FIELD_CONTENT_START ~ FIELD_END) / Find field regions (FIELD_CONTENT_START ~ FIELD_END)
    // control_positions에서 code 3 (FIELD_CONTENT_START)과 code 4 (FIELD_END) 위치 수집
    let mut field_regions: Vec<(usize, usize)> = Vec::new(); // (stream_start, stream_end)

    // control_positions를 정렬하여 순서대로 처리 / Sort control_positions to process in order
    let mut sorted_positions: Vec<_> = control_positions.iter().collect();
    sorted_positions.sort_by_key(|p| p.position);

    let mut current_field_start: Option<usize> = None;
    for pos in &sorted_positions {
        if pos.code == FIELD_CONTENT_START {
            // 필드 콘텐츠 시작 - FIELD_CONTENT_START 제어 문자 다음 위치
            current_field_start = Some(pos.position + get_control_char_size(pos.code));
        } else if pos.code == ControlChar::FIELD_END {
            if let Some(start) = current_field_start.take() {
                // 필드 끝 - FIELD_END 제어 문자 위치까지
                field_regions.push((start, pos.position));
            }
        }
    }

    // 필드 영역 수와 하이퍼링크 수가 다르면 기본 변환 사용 / Use default conversion if counts don't match
    if field_regions.len() != hyperlinks.len() {
        return convert_para_text_to_markdown(text, control_positions);
    }

    let byte_offsets: Vec<usize> = text.char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let char_count = byte_offsets.len() - 1;

    let mut result = String::new();
    let mut current_text_pos = 0;

    // 각 필드 영역 처리 / Process each field region
    for (i, (stream_start, stream_end)) in field_regions.iter().enumerate() {
        // 스트림 위치를 텍스트 위치로 변환 / Convert stream positions to text positions
        let text_start = convert_stream_position_to_text_position(*stream_start, control_positions);
        let text_end = convert_stream_position_to_text_position(*stream_end, control_positions);

        // 필드 이전 텍스트 추가 / Add text before field
        if current_text_pos < text_start && text_start <= char_count {
            let trimmed = &text[byte_offsets[current_text_pos]..byte_offsets[text_start]];
            let trimmed = trimmed.trim();
            if !trimmed.is_empty() {
                result.push_str(trimmed);
            }
        }

        // 필드 내부 텍스트 (하이퍼링크) / Field content (hyperlink)
        if text_start < text_end && text_end <= char_count {
            let trimmed_link = &text[byte_offsets[text_start]..byte_offsets[text_end]];
            let trimmed_link = trimmed_link.trim();
            if !trimmed_link.is_empty() {
                if let Some(hyperlink) = hyperlinks.get(i) {
                    // [text](url) 형식으로 마크다운 링크 생성 / Create markdown link as [text](url)
                    result.push_str(&format!("[{}]({})", trimmed_link, hyperlink.url));
                } else {
                    result.push_str(trimmed_link);
                }
            }
        }

        current_text_pos = text_end;
    }

    // 마지막 필드 이후 텍스트 추가 / Add text after last field
    if current_text_pos < char_count {
        let trimmed = &text[byte_offsets[current_text_pos]..];
        let trimmed = trimmed.trim();
        if !trimmed.is_empty() {
            result.push_str(trimmed);
        }
    }

    let trimmed_result = result.trim();
    if !trimmed_result.is_empty() {
        Some(trimmed_result.to_string())
    } else {
        None
    }
}

/// 문단 경계를 넘는 하이퍼링크 상태
/// Hyperlink state for cross-paragraph hyperlinks
#[derive(Debug, Clone)]
pub struct CrossingHyperlinkState {
    /// 열린 하이퍼링크의 URL / URL of open hyperlink
    pub url: String,
}

/// 문단 경계 하이퍼링크 결과
/// Result of cross-paragraph hyperlink conversion
pub struct CrossingHyperlinkResult {
    /// 마크다운 결과 / Markdown result
    pub markdown: Option<String>,
    /// 새로운 열린 하이퍼링크 상태 (문단 끝에서 하이퍼링크가 열려있으면) / New open hyperlink state (if hyperlink is open at end of paragraph)
    pub new_open_state: Option<CrossingHyperlinkState>,
}

/// 문단 경계를 넘는 하이퍼링크를 처리하여 ParaText를 마크다운으로 변환
/// Convert ParaText to markdown handling cross-paragraph hyperlinks
///
/// # Arguments / 매개변수
/// * `text` - 텍스트 내용 / Text content
/// * `control_positions` - 제어 문자 위치 정보 / Control character positions
/// * `hyperlinks` - 하이퍼링크 정보 리스트 (순서대로) / Hyperlink information list (in order)
/// * `open_hyperlink` - 이전 문단에서 열린 하이퍼링크 상태 / Open hyperlink state from previous paragraph
///
/// # Returns / 반환값
/// 마크다운 문자열과 새로운 열린 하이퍼링크 상태 / Markdown string and new open hyperlink state
pub fn convert_para_text_to_markdown_with_crossing_hyperlinks(
    text: &str,
    control_positions: &[ControlCharPosition],
    hyperlinks: &[HyperlinkRegion],
    open_hyperlink: Option<&CrossingHyperlinkState>,
) -> CrossingHyperlinkResult {
    let byte_offsets: Vec<usize> = text.char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let char_count = byte_offsets.len() - 1;

    // 텍스트가 비어있으면 빈 결과 반환 / Return empty result if text is empty
    if text.trim().is_empty() {
        return CrossingHyperlinkResult {
            markdown: None,
            new_open_state: open_hyperlink.cloned(),
        };
    }

    // control_positions를 정렬 / Sort control_positions
    let mut sorted_positions: Vec<_> = control_positions.iter().collect();
    sorted_positions.sort_by_key(|p| p.position);

    // FIELD_CONTENT_START와 FIELD_END 위치 수집 / Collect FIELD_CONTENT_START and FIELD_END positions
    let mut field_starts: Vec<usize> = Vec::new(); // stream position
    let mut field_ends: Vec<usize> = Vec::new(); // stream position

    for pos in &sorted_positions {
        if pos.code == FIELD_CONTENT_START {
            field_starts.push(pos.position + get_control_char_size(pos.code));
        } else if pos.code == ControlChar::FIELD_END {
            field_ends.push(pos.position);
        }
    }

    let mut result = String::new();
    let mut current_text_pos = 0;
    let mut hyperlink_index = 0;
    let mut new_open_state: Option<CrossingHyperlinkState> = None;

    // 열린 하이퍼링크가 있는 경우 처리 / Handle open hyperlink from previous paragraph
    if let Some(open_state) = open_hyperlink {
        if !field_ends.is_empty() {
            // FIELD_END가 있으면 문단 시작부터 첫 FIELD_END까지 하이퍼링크
            // If FIELD_END exists, hyperlink from paragraph start to first FIELD_END
            let stream_end = field_ends[0];
            let text_end = convert_stream_position_to_text_position(stream_end, control_positions);
            let clamped_end = text_end.min(char_count);

            if clamped_end > 0 {
                let trimmed_link = &text[..byte_offsets[clamped_end]];
                let trimmed_link = trimmed_link.trim();
                if !trimmed_link.is_empty() {
                    result.push_str(&format!("[{}]({})", trimmed_link, open_state.url));
                }
            }
            current_text_pos = clamped_end;
            // 첫 FIELD_END 제거 (이미 처리됨) / Remove first FIELD_END (already processed)
            field_ends.remove(0);
        } else {
            // FIELD_END가 없으면 문단 전체를 하이퍼링크로 처리하고 열린 상태 유지
            // If no FIELD_END, treat entire paragraph as hyperlink and keep open state
            let trimmed_link = text.trim();
            if !trimmed_link.is_empty() {
                result.push_str(&format!("[{}]({})", trimmed_link, open_state.url));
            }
            return CrossingHyperlinkResult {
                markdown: if result.is_empty() {
                    None
                } else {
                    Some(result)
                },
                new_open_state: Some(open_state.clone()),
            };
        }
    }

    // 이 문단 내의 완전한 필드 영역 처리 (FIELD_CONTENT_START ~ FIELD_END 쌍)
    // Process complete field regions in this paragraph (FIELD_CONTENT_START ~ FIELD_END pairs)
    let mut start_idx = 0;
    let mut end_idx = 0;

    while start_idx < field_starts.len() && end_idx < field_ends.len() {
        let stream_start = field_starts[start_idx];
        let stream_end = field_ends[end_idx];

        // FIELD_END가 FIELD_CONTENT_START보다 앞에 있으면 건너뜀 (이미 처리된 열린 하이퍼링크)
        // Skip if FIELD_END is before FIELD_CONTENT_START (already processed open hyperlink)
        if stream_end < stream_start {
            end_idx += 1;
            continue;
        }

        let text_start = convert_stream_position_to_text_position(stream_start, control_positions);
        let text_end = convert_stream_position_to_text_position(stream_end, control_positions);
        let clamped_start = text_start.min(char_count);
        let clamped_end = text_end.min(char_count);

        // 필드 이전 텍스트 추가 / Add text before field
        if current_text_pos < clamped_start {
            let trimmed = &text[byte_offsets[current_text_pos]..byte_offsets[clamped_start]];
            let trimmed = trimmed.trim();
            if !trimmed.is_empty() {
                result.push_str(trimmed);
            }
        }

        // 필드 내부 텍스트 (하이퍼링크) / Field content (hyperlink)
        if clamped_start < clamped_end {
            let trimmed_link = &text[byte_offsets[clamped_start]..byte_offsets[clamped_end]];
            let trimmed_link = trimmed_link.trim();
            if !trimmed_link.is_empty() {
                if let Some(hyperlink) = hyperlinks.get(hyperlink_index) {
                    result.push_str(&format!("[{}]({})", trimmed_link, hyperlink.url));
                } else {
                    result.push_str(trimmed_link);
                }
            }
        }

        current_text_pos = clamped_end;
        start_idx += 1;
        end_idx += 1;
        hyperlink_index += 1;
    }

    // 남은 FIELD_CONTENT_START 처리 (FIELD_END 없이 문단이 끝나는 경우)
    // Process remaining FIELD_CONTENT_START (paragraph ends without FIELD_END)
    if start_idx < field_starts.len() {
        let stream_start = field_starts[start_idx];
        let text_start = convert_stream_position_to_text_position(stream_start, control_positions);
        let clamped_start = text_start.min(char_count);

        // 필드 이전 텍스트 추가 / Add text before field
        if current_text_pos < clamped_start {
            let trimmed = &text[byte_offsets[current_text_pos]..byte_offsets[clamped_start]];
            let trimmed = trimmed.trim();
            if !trimmed.is_empty() {
                result.push_str(trimmed);
            }
        }

        // 필드 시작부터 문단 끝까지 하이퍼링크 / Hyperlink from field start to paragraph end
        if clamped_start < char_count {
            let trimmed_link = &text[byte_offsets[clamped_start]..];
            let trimmed_link = trimmed_link.trim();
            if !trimmed_link.is_empty() {
                if let Some(hyperlink) = hyperlinks.get(hyperlink_index) {
                    result.push_str(&format!("[{}]({})", trimmed_link, hyperlink.url));
                    // 열린 하이퍼링크 상태 설정 / Set open hyperlink state
                    new_open_state = Some(CrossingHyperlinkState {
                        url: hyperlink.url.clone(),
                    });
                } else {
                    result.push_str(trimmed_link);
                }
            }
        }
        current_text_pos = char_count;
    }

    // 마지막 필드 이후 텍스트 추가 / Add text after last field
    if current_text_pos < char_count {
        let trimmed = &text[byte_offsets[current_text_pos]..];
        let trimmed = trimmed.trim();
        if !trimmed.is_empty() {
            result.push_str(trimmed);
        }
    }

    let trimmed_result = result.trim();
    CrossingHyperlinkResult {
        markdown: if trimmed_result.is_empty() {
            None
        } else {
            Some(trimmed_result.to_string())
        },
        new_open_state,
    }
}
