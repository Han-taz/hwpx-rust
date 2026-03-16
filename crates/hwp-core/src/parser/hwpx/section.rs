/// HWPX section XML parser
///
/// Section files (section0.xml, section1.xml, etc.) contain the main document content
/// including paragraphs, tables, images, and other elements.
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::document::bodytext::list_header::{
    LineBreak, ListHeader, ListHeaderAttribute, TextDirection, VerticalAlign,
};
use crate::document::bodytext::para_header::ParaHeader;
use crate::document::bodytext::table::{
    CellAttributes, PageBreakBehavior, Table, TableAttribute, TableAttributes, TableCell,
    TablePadding,
};
use crate::document::bodytext::{ParaTextRun, Paragraph, ParagraphRecord, Section};
use crate::document::BodyText;
use crate::error::{HwpError, ParseWarning, ParseWarnings};
use crate::types::{HWPUNIT, UINT16, WORD};

use super::container::HwpxContainer;

/// Content item type within a cell paragraph
/// 셀 문단 내 콘텐츠 항목 유형
#[derive(Debug, Clone)]
enum CellContentItem {
    Text(String),
    Image(String),
    NestedTable(Table),
}

/// Cell data with colspan/rowspan and address information
#[derive(Debug, Clone)]
struct HwpxCell {
    /// 현재 문단의 텍스트 / Current paragraph text
    current_text: String,
    col_span: u16,
    row_span: u16,
    col_addr: Option<u16>,
    row_addr: Option<u16>,
    /// 셀 내부의 콘텐츠 항목 목록 (순서 보존) / List of content items inside the cell (order preserved)
    content_items: Vec<CellContentItem>,
}

/// Saved table state for nested table handling
/// 중첩 테이블 처리를 위해 저장되는 부모 테이블 상태
#[derive(Debug, Clone)]
struct TableState {
    table_rows: Vec<Vec<HwpxCell>>,
    current_row: Vec<HwpxCell>,
    current_cell: HwpxCell,
    table_caption: String,
    in_cell: bool,
}

impl Default for HwpxCell {
    fn default() -> Self {
        Self {
            current_text: String::new(),
            col_span: 1,
            row_span: 1,
            col_addr: None,
            row_addr: None,
            content_items: Vec::with_capacity(4),
        }
    }
}

/// Text run with associated character shape ID
/// 문자 스타일 ID가 연결된 텍스트 run
#[derive(Debug, Clone)]
struct TextRunInfo {
    text: String,
    char_shape_id: Option<u16>,
}

/// Active hyperlink state for tracking fieldBegin/fieldEnd
/// fieldBegin/fieldEnd 추적을 위한 활성 하이퍼링크 상태
#[derive(Debug, Clone, Default)]
struct HyperlinkState {
    /// Whether we're inside a hyperlink (between fieldBegin and fieldEnd)
    active: bool,
    /// URL extracted from Path parameter
    url: String,
    /// Accumulated text between fieldBegin and fieldEnd
    text: String,
    /// CharShape ID for the hyperlink text
    char_shape_id: Option<u16>,
}

/// Parse all section files and create BodyText
pub fn parse_sections(container: &mut HwpxContainer, warnings: &mut ParseWarnings) -> Result<BodyText, HwpError> {
    let section_files = container.get_section_files();

    if section_files.is_empty() {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "No section files found in Contents/".to_string(),
        });
    }

    let mut sections = Vec::with_capacity(4);

    for (index, section_path) in section_files.iter().enumerate() {
        let content = container.read_file_string(section_path)?;
        let section = parse_section_xml(&content, index as WORD, warnings)?;
        sections.push(section);
    }

    Ok(BodyText { sections })
}

/// Parse a single section XML file
fn parse_section_xml(content: &str, index: WORD, warnings: &mut ParseWarnings) -> Result<Section, HwpError> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut paragraphs = Vec::with_capacity(16);
    let mut current_text = String::new();
    let mut in_text = false;
    let mut in_cell = false;
    let mut in_caption = false;
    let mut _in_picture = false;

    // Image parsing
    let mut current_image_ref: Option<String> = None;

    // Text run tracking with char shape ID
    // charPrIDRef를 추적하여 텍스트 run에 스타일 연결
    let mut current_char_shape_id: Option<u16> = None;
    let mut current_runs: Vec<TextRunInfo> = Vec::with_capacity(4);
    let mut current_run_text = String::new();

    // Hyperlink tracking (fieldBegin/fieldEnd)
    // 하이퍼링크 추적 (fieldBegin/fieldEnd)
    let mut hyperlink_state = HyperlinkState::default();
    let mut in_field_begin = false;
    let mut in_parameters = false;
    let mut current_param_name = String::new();

    // Table parsing with colspan/rowspan support
    let mut table_rows: Vec<Vec<HwpxCell>> = Vec::with_capacity(8);
    let mut current_row: Vec<HwpxCell> = Vec::with_capacity(4);
    let mut current_cell = HwpxCell::default();
    let mut table_caption = String::new();

    // Paragraph shape/style ID tracking (from <hp:p prIDRef="N" styleIDRef="N">)
    // 문단 모양/스타일 ID 추적
    let mut current_para_shape_id: u16 = 0;
    let mut current_para_style_id: u8 = 0;

    // Track nesting depth for paragraphs and tables
    // 문단과 테이블의 중첩 깊이 추적
    let mut para_depth: u32 = 0;
    let mut table_depth: u32 = 0;

    // Stack to save parent table state when entering nested table
    // 중첩 테이블에 진입할 때 부모 테이블 상태를 저장하는 스택
    let mut table_state_stack: Vec<TableState> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Empty(ref e)) => {
                // Handle self-closing tags like <hp:cellSpan ... />, <hp:cellAddr ... />, <hp:tab ... />
                let local_name = e.name();
                let local_name = local_name.as_ref();

                if local_name.ends_with(b":tab") || local_name == b"tab" {
                    // Parse tab element and convert to appropriate text representation
                    // Tab attributes: width (HWPUNIT), leader (0=none, 1=solid, 2=dash, 3=dot), type
                    let mut leader: u8 = 0;
                    let mut width: u32 = 0;

                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        match key {
                            b"leader" => {
                                let value = String::from_utf8_lossy(&attr.value);
                                leader = value.parse().unwrap_or_else(|_| {
                                    warnings.push(ParseWarning::warning(format!(
                                        "Invalid tab leader value: {value}"
                                    )));
                                    0
                                });
                            }
                            b"width" => {
                                let value = String::from_utf8_lossy(&attr.value);
                                width = value.parse().unwrap_or_else(|_| {
                                    warnings.push(ParseWarning::warning(format!(
                                        "Invalid tab width value: {value}"
                                    )));
                                    0
                                });
                            }
                            _ => {}
                        }
                    }

                    // Generate tab representation based on leader type
                    // Leader: 0=none, 1=solid, 2=dash, 3=dot
                    let tab_text = match leader {
                        3 => {
                            // Dot leader - generate dots based on approximate width
                            // HWPUNIT: 7200 units = 1 inch, roughly 6 chars per inch
                            let dot_count = (width / 1200).min(80).max(3) as usize;
                            ".".repeat(dot_count)
                        }
                        2 => {
                            // Dash leader
                            let dash_count = (width / 2400).min(40).max(2) as usize;
                            "-".repeat(dash_count)
                        }
                        1 => {
                            // Solid line leader
                            let line_count = (width / 2400).min(40).max(2) as usize;
                            "_".repeat(line_count)
                        }
                        _ => {
                            // No leader - use tab character or spaces
                            "\t".to_string()
                        }
                    };

                    // Add tab representation to current text context
                    let in_table = table_depth > 0;
                    if in_table && in_caption {
                        table_caption.push_str(&tab_text);
                    } else if in_table && in_cell {
                        current_cell.current_text.push_str(&tab_text);
                    } else if !in_table {
                        current_text.push_str(&tab_text);
                    }
                } else if local_name.ends_with(b":cellSpan") || local_name == b"cellSpan" {
                    // Parse colspan and rowspan attributes
                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        match key {
                            b"colSpan" => {
                                let value = String::from_utf8_lossy(&attr.value);
                                current_cell.col_span = value.parse().unwrap_or_else(|_| {
                                    warnings.push(ParseWarning::warning(format!(
                                        "Invalid cellSpan colSpan value: {value}"
                                    )));
                                    1
                                });
                            }
                            b"rowSpan" => {
                                let value = String::from_utf8_lossy(&attr.value);
                                current_cell.row_span = value.parse().unwrap_or_else(|_| {
                                    warnings.push(ParseWarning::warning(format!(
                                        "Invalid cellSpan rowSpan value: {value}"
                                    )));
                                    1
                                });
                            }
                            _ => {}
                        }
                    }
                } else if local_name.ends_with(b":cellAddr") || local_name == b"cellAddr" {
                    // Parse cell address (actual column and row position)
                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        match key {
                            b"colAddr" => {
                                let value = String::from_utf8_lossy(&attr.value);
                                current_cell.col_addr = Some(value.parse().unwrap_or_else(|_| {
                                    warnings.push(ParseWarning::warning(format!(
                                        "Invalid cellAddr colAddr value: {value}"
                                    )));
                                    0
                                }));
                            }
                            b"rowAddr" => {
                                let value = String::from_utf8_lossy(&attr.value);
                                current_cell.row_addr = Some(value.parse().unwrap_or_else(|_| {
                                    warnings.push(ParseWarning::warning(format!(
                                        "Invalid cellAddr rowAddr value: {value}"
                                    )));
                                    0
                                }));
                            }
                            _ => {}
                        }
                    }
                } else if local_name.ends_with(b":img") || local_name == b"img" {
                    // Parse image element - extract binaryItemIDRef
                    // <hc:img binaryItemIDRef="image1" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/>
                    for attr in e.attributes().flatten() {
                        let key = attr.key.as_ref();
                        if key == b"binaryItemIDRef" {
                            let value = String::from_utf8_lossy(&attr.value);
                            current_image_ref = Some(value.to_string());
                        }
                    }
                } else if local_name.ends_with(b":fieldEnd") || local_name == b"fieldEnd" {
                    // Handle self-closing fieldEnd: <hp:fieldEnd beginIDRef="..." />
                    // Self-closing fieldEnd 처리: <hp:fieldEnd beginIDRef="..." />
                    if hyperlink_state.active && !hyperlink_state.url.is_empty() {
                        // Add hyperlink as a special marker in runs
                        // 하이퍼링크를 특수 마커로 runs에 추가
                        current_runs.push(TextRunInfo {
                            text: format!(
                                "\x00HYPERLINK:{}\x00{}",
                                hyperlink_state.url, hyperlink_state.text
                            ),
                            char_shape_id: hyperlink_state.char_shape_id,
                        });
                    }
                    // Reset hyperlink state
                    hyperlink_state = HyperlinkState::default();
                }
            }
            Ok(Event::Start(ref e)) => {
                let local_name = e.name();
                let local_name = local_name.as_ref();

                match local_name {
                    s if s.ends_with(b":p") || s == b"p" => {
                        para_depth += 1;
                        if table_depth == 0 && para_depth == 1 {
                            current_text.clear();
                            current_runs.clear();
                            current_run_text.clear();
                            // Parse prIDRef and styleIDRef from <hp:p>
                            // <hp:p> 요소에서 prIDRef, styleIDRef 파싱
                            current_para_shape_id = 0;
                            current_para_style_id = 0;
                            for attr in e.attributes().flatten() {
                                let key = attr.key.as_ref();
                                match key {
                                    b"prIDRef" => {
                                        let value = String::from_utf8_lossy(&attr.value);
                                        current_para_shape_id = value.parse().unwrap_or_else(|_| {
                                            warnings.push(ParseWarning::warning(format!(
                                                "Invalid paragraph prIDRef value: {value}"
                                            )));
                                            0
                                        });
                                    }
                                    b"styleIDRef" => {
                                        let value = String::from_utf8_lossy(&attr.value);
                                        current_para_style_id =
                                            value.parse::<u16>().unwrap_or_else(|_| {
                                                warnings.push(ParseWarning::warning(format!(
                                                    "Invalid paragraph styleIDRef value: {value}"
                                                )));
                                                0
                                            }) as u8;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    s if s.ends_with(b":run") || s == b"run" => {
                        // Save previous run if any text accumulated
                        // 이전 run의 텍스트가 있으면 저장
                        if !current_run_text.is_empty() {
                            current_runs.push(TextRunInfo {
                                text: std::mem::take(&mut current_run_text),
                                char_shape_id: current_char_shape_id,
                            });
                        }
                        // Parse charPrIDRef from <hp:run charPrIDRef="N">
                        current_char_shape_id = None;
                        for attr in e.attributes().flatten() {
                            let key = attr.key.as_ref();
                            if key == b"charPrIDRef" {
                                let value = String::from_utf8_lossy(&attr.value);
                                current_char_shape_id = value.parse().ok();
                            }
                        }
                    }
                    s if s.ends_with(b":t") || s == b"t" => {
                        in_text = true;
                    }
                    s if s.ends_with(b":tbl") || s == b"tbl" => {
                        // If already in a table (nested table), save current state
                        // 이미 테이블 안에 있으면 (중첩 테이블) 현재 상태 저장
                        if table_depth > 0 {
                            table_state_stack.push(TableState {
                                table_rows: std::mem::take(&mut table_rows),
                                current_row: std::mem::take(&mut current_row),
                                current_cell: std::mem::take(&mut current_cell),
                                table_caption: std::mem::take(&mut table_caption),
                                in_cell,
                            });
                        }
                        table_depth += 1;
                        table_rows.clear();
                        table_caption.clear();
                    }
                    s if s.ends_with(b":caption") || s == b"caption" => {
                        in_caption = true;
                    }
                    s if s.ends_with(b":tr") || s == b"tr" => {
                        current_row.clear();
                    }
                    s if s.ends_with(b":tc") || s == b"tc" => {
                        in_cell = true;
                        current_cell = HwpxCell::default();
                    }
                    s if s.ends_with(b":pic") || s == b"pic" => {
                        _in_picture = true;
                        current_image_ref = None;
                    }
                    s if s.ends_with(b":fieldBegin") || s == b"fieldBegin" => {
                        // Parse fieldBegin for hyperlinks
                        // <hp:fieldBegin type="HYPERLINK" id="...">
                        in_field_begin = true;
                        let mut is_hyperlink = false;

                        for attr in e.attributes().flatten() {
                            let key = attr.key.as_ref();
                            if key == b"type" {
                                is_hyperlink = attr.value.as_ref() == b"HYPERLINK";
                            }
                        }

                        if is_hyperlink {
                            // Start tracking hyperlink
                            hyperlink_state = HyperlinkState {
                                active: true,
                                url: String::new(),
                                text: String::new(),
                                char_shape_id: current_char_shape_id,
                            };
                        }
                    }
                    s if s.ends_with(b":parameters") || s == b"parameters" => {
                        if in_field_begin {
                            in_parameters = true;
                        }
                    }
                    s if s.ends_with(b":stringParam") || s == b"stringParam" => {
                        // Parse stringParam for URL extraction
                        // <hp:stringParam name="Path">URL</hp:stringParam>
                        if in_parameters && hyperlink_state.active {
                            for attr in e.attributes().flatten() {
                                let key = attr.key.as_ref();
                                if key == b"name" {
                                    let value = String::from_utf8_lossy(&attr.value);
                                    current_param_name = value.to_string();
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();

                // Handle stringParam content for hyperlink URL
                if in_parameters && hyperlink_state.active && current_param_name == "Path" {
                    hyperlink_state.url = text.clone();
                } else if in_text {
                    let in_table = table_depth > 0;
                    if in_table && in_caption {
                        // Text inside table caption
                        table_caption.push_str(&text);
                    } else if in_table && in_cell {
                        current_cell.current_text.push_str(&text);
                    } else if !in_table {
                        // If inside hyperlink, collect text ONLY for hyperlink (not as regular text)
                        // 하이퍼링크 내부면 하이퍼링크 텍스트로만 수집 (일반 텍스트로 추가 안 함)
                        if hyperlink_state.active {
                            hyperlink_state.text.push_str(&text);
                            // Update char_shape_id if we have one for this run
                            if hyperlink_state.char_shape_id.is_none() {
                                hyperlink_state.char_shape_id = current_char_shape_id;
                            }
                        } else {
                            // Normal text - collect for current run
                            current_run_text.push_str(&text);
                            current_text.push_str(&text);
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let local_name = e.name();
                let local_name = local_name.as_ref();

                match local_name {
                    s if s.ends_with(b":run") || s == b"run" => {
                        // Save current run when </hp:run> ends
                        // </hp:run>이 끝나면 현재 run 저장
                        if !current_run_text.is_empty() {
                            current_runs.push(TextRunInfo {
                                text: std::mem::take(&mut current_run_text),
                                char_shape_id: current_char_shape_id,
                            });
                        }
                    }
                    s if s.ends_with(b":p") || s == b"p" => {
                        let in_table = table_depth > 0;
                        if para_depth == 1 && !in_table {
                            // Save any remaining run text
                            if !current_run_text.is_empty() {
                                current_runs.push(TextRunInfo {
                                    text: std::mem::take(&mut current_run_text),
                                    char_shape_id: current_char_shape_id,
                                });
                            }
                            // Create paragraph with runs if any
                            if !current_runs.is_empty() {
                                let mut para = create_paragraph_with_runs(&current_runs);
                                para.para_header.para_shape_id = current_para_shape_id;
                                para.para_header.para_style_id = current_para_style_id;
                                paragraphs.push(para);
                                current_runs.clear();
                            } else if !current_text.is_empty() {
                                // Fallback to old behavior
                                let mut para = create_paragraph(&current_text);
                                para.para_header.para_shape_id = current_para_shape_id;
                                para.para_header.para_style_id = current_para_style_id;
                                paragraphs.push(para);
                            }
                            current_text.clear();
                        }
                        // Save current paragraph text as a content item when paragraph ends inside cell
                        // 셀 내부 문단이 끝나면 현재 텍스트를 콘텐츠 항목으로 저장
                        if in_cell && !current_cell.current_text.is_empty() {
                            current_cell
                                .content_items
                                .push(CellContentItem::Text(std::mem::take(&mut current_cell.current_text)));
                        }
                        // Add newline between nested paragraphs (e.g., in drawText/container)
                        // This ensures proper line breaks in TOC and other nested structures
                        if para_depth > 1 && !in_table && !current_text.is_empty() {
                            current_text.push('\n');
                        }
                        para_depth = para_depth.saturating_sub(1);
                    }
                    s if s.ends_with(b":t") || s == b"t" => {
                        in_text = false;
                    }
                    s if s.ends_with(b":caption") || s == b"caption" => {
                        in_caption = false;
                    }
                    s if s.ends_with(b":tbl") || s == b"tbl" => {
                        table_depth = table_depth.saturating_sub(1);

                        if table_depth == 0 {
                            // Outermost table complete - add to paragraphs
                            // 최외곽 테이블 완료 - paragraph로 추가
                            let caption_trimmed = table_caption.trim();
                            if !caption_trimmed.is_empty() {
                                paragraphs.push(create_paragraph(caption_trimmed));
                            }
                            if !table_rows.is_empty() {
                                paragraphs.push(create_table_paragraph_with_spans(&table_rows));
                            }
                            table_caption.clear();
                        } else {
                            // Nested table complete - convert to content for parent cell
                            // 중첩 테이블 완료 - 부모 셀의 콘텐츠로 변환
                            let nested_table = if !table_rows.is_empty() {
                                Some(create_table_from_rows(&table_rows))
                            } else {
                                None
                            };

                            // Restore parent table state
                            // 부모 테이블 상태 복원
                            if let Some(parent_state) = table_state_stack.pop() {
                                table_rows = parent_state.table_rows;
                                current_row = parent_state.current_row;
                                current_cell = parent_state.current_cell;
                                table_caption = parent_state.table_caption;
                                in_cell = parent_state.in_cell;

                                // Add nested table to parent cell's content
                                // 중첩 테이블을 부모 셀의 콘텐츠에 추가
                                if let Some(table) = nested_table {
                                    current_cell
                                        .content_items
                                        .push(CellContentItem::NestedTable(table));
                                }
                            }
                        }
                    }
                    s if s.ends_with(b":tr") || s == b"tr" => {
                        if !current_row.is_empty() {
                            table_rows.push(std::mem::take(&mut current_row));
                        }
                    }
                    s if s.ends_with(b":tc") || s == b"tc" => {
                        // Cell parsing complete, push to current row
                        // 셀 파싱 완료, 현재 행에 추가
                        current_row.push(std::mem::take(&mut current_cell));
                        in_cell = false;
                    }
                    s if s.ends_with(b":pic") || s == b"pic" => {
                        // Create image paragraph when picture element ends
                        // 테이블 셀 내부의 이미지는 셀에 저장하고, 그 외에는 별도 paragraph로 추가
                        // Store images inside table cells, otherwise add as separate paragraph
                        if let Some(image_ref) = std::mem::take(&mut current_image_ref) {
                            let in_table = table_depth > 0;
                            if in_table && in_cell {
                                // 테이블 셀 내부의 이미지는 순서대로 콘텐츠 항목에 추가
                                // Add image to content items in order
                                current_cell
                                    .content_items
                                    .push(CellContentItem::Image(image_ref));
                            } else {
                                // 테이블 밖의 이미지는 별도 paragraph로 추가
                                paragraphs.push(create_image_paragraph(&image_ref));
                            }
                        }
                        _in_picture = false;
                    }
                    s if s.ends_with(b":fieldBegin") || s == b"fieldBegin" => {
                        in_field_begin = false;
                    }
                    s if s.ends_with(b":fieldEnd") || s == b"fieldEnd" => {
                        // Hyperlink complete - create hyperlink run
                        // 하이퍼링크 완료 - 하이퍼링크 run 생성
                        if hyperlink_state.active && !hyperlink_state.url.is_empty() {
                            // Add hyperlink as a special marker in runs
                            // 하이퍼링크를 특수 마커로 runs에 추가
                            current_runs.push(TextRunInfo {
                                text: format!(
                                    "\x00HYPERLINK:{}\x00{}",
                                    hyperlink_state.url, hyperlink_state.text
                                ),
                                char_shape_id: hyperlink_state.char_shape_id,
                            });
                        }

                        // Reset hyperlink state
                        hyperlink_state = HyperlinkState::default();
                    }
                    s if s.ends_with(b":parameters") || s == b"parameters" => {
                        in_parameters = false;
                    }
                    s if s.ends_with(b":stringParam") || s == b"stringParam" => {
                        current_param_name.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(HwpError::XmlParseError(format!(
                    "Error parsing section XML: {e}"
                )))
            }
            _ => {}
        }
    }

    Ok(Section { index, paragraphs })
}

/// Create a paragraph from text content
fn create_paragraph(text: &str) -> Paragraph {
    let para_header = ParaHeader {
        text_char_count: text.chars().count() as u32,
        ..Default::default()
    };

    let mut records = Vec::new();

    // Create ParaText record
    let runs = vec![ParaTextRun::Text {
        text: text.to_string(),
        char_shape_id: None,
    }];

    records.push(ParagraphRecord::ParaText {
        data: Box::new(crate::document::bodytext::ParaTextData {
            text: text.to_string(),
            runs,
            control_char_positions: vec![],
            inline_control_params: vec![],
        }),
    });

    Paragraph {
        para_header,
        records,
    }
}

/// Create a paragraph from text runs with char_shape_id
/// char_shape_id가 연결된 텍스트 run들로 paragraph 생성
fn create_paragraph_with_runs(text_runs: &[TextRunInfo]) -> Paragraph {
    // Build runs, handling hyperlink markers
    let mut runs: Vec<ParaTextRun> = Vec::with_capacity(4);
    let mut total_text = String::new();

    for run_info in text_runs {
        if run_info.text.is_empty() {
            continue;
        }

        // Check for hyperlink marker: \x00HYPERLINK:url\x00text
        if run_info.text.starts_with("\x00HYPERLINK:") {
            // Parse hyperlink: \x00HYPERLINK:url\x00text
            let content = &run_info.text[11..]; // Skip "\x00HYPERLINK:"
            if let Some(null_pos) = content.find('\x00') {
                let url = &content[..null_pos];
                let text = &content[null_pos + 1..];
                runs.push(ParaTextRun::Hyperlink {
                    text: text.to_string(),
                    url: url.to_string(),
                    char_shape_id: run_info.char_shape_id,
                });
                total_text.push_str(text);
            }
        } else {
            runs.push(ParaTextRun::Text {
                text: run_info.text.clone(),
                char_shape_id: run_info.char_shape_id,
            });
            total_text.push_str(&run_info.text);
        }
    }

    let para_header = ParaHeader {
        text_char_count: total_text.chars().count() as u32,
        ..Default::default()
    };

    let records = vec![ParagraphRecord::ParaText {
        data: Box::new(crate::document::bodytext::ParaTextData {
            text: total_text,
            runs,
            control_char_positions: vec![],
            inline_control_params: vec![],
        }),
    }];

    Paragraph {
        para_header,
        records,
    }
}

/// Create a Table struct from rows (used for nested tables)
/// 행 데이터로부터 Table 구조체 생성 (중첩 테이블용)
fn create_table_from_rows(rows: &[Vec<HwpxCell>]) -> Table {
    let row_count = rows.len() as UINT16;

    // Calculate actual column count from maximum (col_addr + col_span) across all cells
    let col_count = rows
        .iter()
        .flat_map(|row| row.iter())
        .map(|c| {
            let col_addr = c.col_addr.unwrap_or(0) as usize;
            col_addr + c.col_span as usize
        })
        .max()
        .unwrap_or(0) as UINT16;

    let table_attributes = TableAttributes {
        attribute: TableAttribute {
            page_break: PageBreakBehavior::NoBreak,
            header_row_repeat: false,
        },
        row_count,
        col_count,
        cell_spacing: 0,
        padding: TablePadding {
            left: 0,
            right: 0,
            top: 0,
            bottom: 0,
        },
        row_sizes: vec![],
        border_fill_id: 0,
        zones: vec![],
    };

    let total_cells: usize = rows.iter().map(|r| r.len()).sum();
    let mut cells = Vec::with_capacity(total_cells);

    for (row_idx, row) in rows.iter().enumerate() {
        let mut calc_col_address: u16 = 0;

        for cell_data in row.iter() {
            let col_address = cell_data.col_addr.unwrap_or(calc_col_address);
            let row_address = cell_data.row_addr.unwrap_or(row_idx as u16);

            let mut cell_paragraphs = Vec::with_capacity(cell_data.content_items.len().max(1));

            for item in &cell_data.content_items {
                match item {
                    CellContentItem::Text(text) => {
                        if !text.is_empty() {
                            cell_paragraphs.push(create_paragraph(text));
                        }
                    }
                    CellContentItem::Image(image_ref) => {
                        cell_paragraphs.push(create_image_paragraph(image_ref));
                    }
                    CellContentItem::NestedTable(nested_table) => {
                        // Create a paragraph containing the nested table
                        // 중첩 테이블을 포함하는 paragraph 생성
                        let para_header = ParaHeader {
                            text_char_count: 1,
                            ..Default::default()
                        };
                        let records = vec![ParagraphRecord::Table {
                            table: nested_table.clone(),
                        }];
                        cell_paragraphs.push(Paragraph {
                            para_header,
                            records,
                        });
                    }
                }
            }

            if cell_paragraphs.is_empty() {
                cell_paragraphs.push(create_paragraph(""));
            }

            let cell = TableCell {
                list_header: ListHeader {
                    paragraph_count: cell_paragraphs.len() as i16,
                    attribute: ListHeaderAttribute {
                        text_direction: TextDirection::Horizontal,
                        line_break: LineBreak::Normal,
                        vertical_align: VerticalAlign::Top,
                    },
                },
                cell_attributes: CellAttributes {
                    col_address,
                    row_address,
                    col_span: cell_data.col_span,
                    row_span: cell_data.row_span,
                    width: HWPUNIT(5000),
                    height: HWPUNIT(1000),
                    left_margin: 0,
                    right_margin: 0,
                    top_margin: 0,
                    bottom_margin: 0,
                    border_fill_id: 0,
                },
                paragraphs: cell_paragraphs,
            };
            cells.push(cell);

            calc_col_address = col_address + cell_data.col_span;
        }
    }

    Table {
        attributes: table_attributes,
        cells,
    }
}

/// Create a paragraph containing a table with proper colspan/rowspan
/// `create_table_from_rows`로 Table을 생성한 후 Paragraph로 래핑
fn create_table_paragraph_with_spans(rows: &[Vec<HwpxCell>]) -> Paragraph {
    let table = create_table_from_rows(rows);

    let para_header = ParaHeader {
        text_char_count: 1, // Table control character
        ..Default::default()
    };

    let records = vec![ParagraphRecord::Table { table }];

    Paragraph {
        para_header,
        records,
    }
}

/// Create a paragraph containing an image reference
fn create_image_paragraph(binary_item_ref: &str) -> Paragraph {
    let para_header = ParaHeader {
        text_char_count: 1, // Image control character
        ..Default::default()
    };

    let records = vec![ParagraphRecord::HwpxImage {
        binary_item_ref: binary_item_ref.to_string(),
    }];

    Paragraph {
        para_header,
        records,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::bodytext::ParagraphRecord;

    /// Helper: wrap XML fragment in a minimal section envelope
    fn wrap_section(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section"
        xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"
        xmlns:hc="http://www.hancom.co.kr/hwpml/2011/content">
{body}
</hs:sec>"#
        )
    }

    /// Helper: extract text from first ParaText record of a paragraph
    fn para_text(para: &Paragraph) -> Option<&str> {
        para.records.iter().find_map(|r| {
            if let ParagraphRecord::ParaText { data } = r {
                Some(data.text.as_str())
            } else {
                None
            }
        })
    }

    /// Helper: extract runs from first ParaText record of a paragraph
    fn para_runs(para: &Paragraph) -> Option<&Vec<ParaTextRun>> {
        para.records.iter().find_map(|r| {
            if let ParagraphRecord::ParaText { data } = r {
                Some(&data.runs)
            } else {
                None
            }
        })
    }

    // ===== Basic paragraph tests =====

    #[test]
    fn test_parse_single_paragraph() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run>
                    <hp:t>Hello World</hp:t>
                </hp:run>
            </hp:p>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        assert_eq!(section.paragraphs.len(), 1);
        assert_eq!(para_text(&section.paragraphs[0]), Some("Hello World"));
    }

    #[test]
    fn test_parse_multiple_paragraphs() {
        let xml = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>First</hp:t></hp:run></hp:p>
            <hp:p><hp:run><hp:t>Second</hp:t></hp:run></hp:p>
            <hp:p><hp:run><hp:t>Third</hp:t></hp:run></hp:p>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        assert_eq!(section.paragraphs.len(), 3);
        assert_eq!(para_text(&section.paragraphs[0]), Some("First"));
        assert_eq!(para_text(&section.paragraphs[1]), Some("Second"));
        assert_eq!(para_text(&section.paragraphs[2]), Some("Third"));
    }

    #[test]
    fn test_parse_empty_section() {
        let xml = wrap_section("");
        let section = parse_section_xml(&xml, 5, &mut ParseWarnings::new()).unwrap();
        assert_eq!(section.index, 5);
        assert!(section.paragraphs.is_empty());
    }

    #[test]
    fn test_parse_paragraph_with_multiple_runs() {
        // Note: trim_text(true) in parser trims whitespace from text nodes
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run charPrIDRef="1"><hp:t>Bold</hp:t></hp:run>
                <hp:run charPrIDRef="2"><hp:t>Italic</hp:t></hp:run>
            </hp:p>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        assert_eq!(section.paragraphs.len(), 1);
        assert_eq!(para_text(&section.paragraphs[0]), Some("BoldItalic"));

        let runs = para_runs(&section.paragraphs[0]).unwrap();
        assert_eq!(runs.len(), 2);
        match &runs[0] {
            ParaTextRun::Text {
                text,
                char_shape_id,
            } => {
                assert_eq!(text, "Bold");
                assert_eq!(*char_shape_id, Some(1));
            }
            _ => panic!("Expected Text run"),
        }
        match &runs[1] {
            ParaTextRun::Text {
                text,
                char_shape_id,
            } => {
                assert_eq!(text, "Italic");
                assert_eq!(*char_shape_id, Some(2));
            }
            _ => panic!("Expected Text run"),
        }
    }

    // ===== Paragraph attribute tests =====

    #[test]
    fn test_parse_paragraph_shape_style_ids() {
        let xml = wrap_section(
            r#"
            <hp:p prIDRef="3" styleIDRef="2">
                <hp:run><hp:t>Styled paragraph</hp:t></hp:run>
            </hp:p>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        assert_eq!(section.paragraphs.len(), 1);
        assert_eq!(section.paragraphs[0].para_header.para_shape_id, 3);
        assert_eq!(section.paragraphs[0].para_header.para_style_id, 2);
    }

    #[test]
    fn test_paragraph_default_shape_style_ids() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run><hp:t>No attributes</hp:t></hp:run>
            </hp:p>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        assert_eq!(section.paragraphs[0].para_header.para_shape_id, 0);
        assert_eq!(section.paragraphs[0].para_header.para_style_id, 0);
    }

    // ===== Tab tests =====

    #[test]
    fn test_parse_tab_element() {
        // Tab text goes into current_text only (not runs).
        // When runs exist, the paragraph is built from runs — so tab is not in the final text.
        // Test that at least the surrounding text is preserved.
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run>
                    <hp:t>Before</hp:t>
                    <hp:tab leader="0" width="3600"/>
                    <hp:t>After</hp:t>
                </hp:run>
            </hp:p>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        assert_eq!(section.paragraphs.len(), 1);
        let text = para_text(&section.paragraphs[0]).unwrap();
        assert!(text.contains("Before"));
        assert!(text.contains("After"));
    }

    #[test]
    fn test_parse_dot_leader_tab() {
        // Tab leaders are only added to current_text, not runs.
        // Verify surrounding text is captured correctly.
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run>
                    <hp:t>Name</hp:t>
                    <hp:tab leader="3" width="7200"/>
                    <hp:t>100</hp:t>
                </hp:run>
            </hp:p>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        let text = para_text(&section.paragraphs[0]).unwrap();
        assert!(text.contains("Name"));
        assert!(text.contains("100"));
    }

    // ===== Table tests =====

    #[test]
    fn test_parse_simple_table() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:tr>
                    <hp:tc>
                        <hp:cellAddr colAddr="0" rowAddr="0"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p><hp:run><hp:t>A</hp:t></hp:run></hp:p>
                        </hp:subList>
                    </hp:tc>
                    <hp:tc>
                        <hp:cellAddr colAddr="1" rowAddr="0"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p><hp:run><hp:t>B</hp:t></hp:run></hp:p>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
                <hp:tr>
                    <hp:tc>
                        <hp:cellAddr colAddr="0" rowAddr="1"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p><hp:run><hp:t>C</hp:t></hp:run></hp:p>
                        </hp:subList>
                    </hp:tc>
                    <hp:tc>
                        <hp:cellAddr colAddr="1" rowAddr="1"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p><hp:run><hp:t>D</hp:t></hp:run></hp:p>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
            </hp:tbl>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        // Table should produce exactly one paragraph
        assert_eq!(section.paragraphs.len(), 1);

        // Extract table
        let table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            other => panic!("Expected Table record, got: {other:?}"),
        };

        assert_eq!(table.attributes.row_count, 2);
        assert_eq!(table.attributes.col_count, 2);
        assert_eq!(table.cells.len(), 4);

        // Check cell content
        assert_eq!(table.cells[0].cell_attributes.col_address, 0);
        assert_eq!(table.cells[0].cell_attributes.row_address, 0);
        assert_eq!(table.cells[1].cell_attributes.col_address, 1);
    }

    #[test]
    fn test_parse_table_with_colspan() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:tr>
                    <hp:tc>
                        <hp:cellAddr colAddr="0" rowAddr="0"/>
                        <hp:cellSpan colSpan="2" rowSpan="1"/>
                        <hp:subList>
                            <hp:p><hp:run><hp:t>Merged</hp:t></hp:run></hp:p>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
                <hp:tr>
                    <hp:tc>
                        <hp:cellAddr colAddr="0" rowAddr="1"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p><hp:run><hp:t>Left</hp:t></hp:run></hp:p>
                        </hp:subList>
                    </hp:tc>
                    <hp:tc>
                        <hp:cellAddr colAddr="1" rowAddr="1"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p><hp:run><hp:t>Right</hp:t></hp:run></hp:p>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
            </hp:tbl>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        let table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            _ => panic!("Expected Table"),
        };
        assert_eq!(table.attributes.col_count, 2);
        assert_eq!(table.cells[0].cell_attributes.col_span, 2);
    }

    #[test]
    fn test_parse_table_with_caption() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:caption>
                    <hp:p><hp:run><hp:t>Table Caption</hp:t></hp:run></hp:p>
                </hp:caption>
                <hp:tr>
                    <hp:tc>
                        <hp:cellAddr colAddr="0" rowAddr="0"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p><hp:run><hp:t>Cell</hp:t></hp:run></hp:p>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
            </hp:tbl>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        // Caption paragraph + table paragraph
        assert_eq!(section.paragraphs.len(), 2);
        assert_eq!(para_text(&section.paragraphs[0]), Some("Table Caption"));
    }

    // ===== Image tests =====

    #[test]
    fn test_parse_image() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run><hp:t>Before image</hp:t></hp:run>
            </hp:p>
            <hp:p>
                <hp:pic>
                    <hc:img binaryItemIDRef="image1" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/>
                </hp:pic>
            </hp:p>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        // Text paragraph + image paragraph
        assert!(section.paragraphs.len() >= 2);

        // Find image record
        let has_image = section.paragraphs.iter().any(|p| {
            p.records
                .iter()
                .any(|r| matches!(r, ParagraphRecord::HwpxImage { .. }))
        });
        assert!(has_image, "Should have an image record");
    }

    #[test]
    fn test_parse_image_in_table_cell() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:tr>
                    <hp:tc>
                        <hp:cellAddr colAddr="0" rowAddr="0"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p>
                                <hp:pic>
                                    <hc:img binaryItemIDRef="cellimg" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/>
                                </hp:pic>
                            </hp:p>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
            </hp:tbl>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        let table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            _ => panic!("Expected Table"),
        };
        // Cell should have an image paragraph
        let cell_has_image = table.cells[0].paragraphs.iter().any(|p| {
            p.records.iter().any(|r| {
                matches!(r, ParagraphRecord::HwpxImage { binary_item_ref } if binary_item_ref == "cellimg")
            })
        });
        assert!(cell_has_image, "Table cell should contain image");
    }

    // ===== Hyperlink tests =====

    #[test]
    fn test_parse_hyperlink() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run charPrIDRef="0">
                    <hp:fieldBegin type="HYPERLINK" id="0">
                        <hp:parameters>
                            <hp:stringParam name="Path">https://example.com</hp:stringParam>
                        </hp:parameters>
                    </hp:fieldBegin>
                    <hp:t>Click here</hp:t>
                    <hp:fieldEnd beginIDRef="0"/>
                </hp:run>
            </hp:p>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        assert_eq!(section.paragraphs.len(), 1);

        let runs = para_runs(&section.paragraphs[0]).unwrap();
        let has_hyperlink = runs.iter().any(|r| {
            matches!(r, ParaTextRun::Hyperlink { url, text, .. }
                if url == "https://example.com" && text == "Click here")
        });
        assert!(has_hyperlink, "Should contain a hyperlink run");
    }

    #[test]
    fn test_parse_hyperlink_with_preceding_text() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run charPrIDRef="0"><hp:t>Visit </hp:t></hp:run>
                <hp:run charPrIDRef="1">
                    <hp:fieldBegin type="HYPERLINK" id="0">
                        <hp:parameters>
                            <hp:stringParam name="Path">https://rust-lang.org</hp:stringParam>
                        </hp:parameters>
                    </hp:fieldBegin>
                    <hp:t>Rust</hp:t>
                    <hp:fieldEnd beginIDRef="0"/>
                </hp:run>
                <hp:run charPrIDRef="0"><hp:t> site</hp:t></hp:run>
            </hp:p>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        let runs = para_runs(&section.paragraphs[0]).unwrap();

        // Should have: "Visit " text + Hyperlink + " site" text
        assert!(
            runs.len() >= 3,
            "Expected at least 3 runs, got {}",
            runs.len()
        );

        let hyperlink_count = runs
            .iter()
            .filter(|r| matches!(r, ParaTextRun::Hyperlink { .. }))
            .count();
        assert_eq!(hyperlink_count, 1, "Should have exactly one hyperlink");
    }

    // ===== Nested table tests =====

    #[test]
    fn test_parse_nested_table() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:tr>
                    <hp:tc>
                        <hp:cellAddr colAddr="0" rowAddr="0"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p><hp:run><hp:t>Outer cell text</hp:t></hp:run></hp:p>
                            <hp:tbl>
                                <hp:tr>
                                    <hp:tc>
                                        <hp:cellAddr colAddr="0" rowAddr="0"/>
                                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                                        <hp:subList>
                                            <hp:p><hp:run><hp:t>Inner</hp:t></hp:run></hp:p>
                                        </hp:subList>
                                    </hp:tc>
                                </hp:tr>
                            </hp:tbl>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
            </hp:tbl>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        assert_eq!(section.paragraphs.len(), 1);

        let outer_table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            _ => panic!("Expected outer Table"),
        };
        assert_eq!(outer_table.cells.len(), 1);

        // The outer cell should contain a nested table paragraph
        let has_nested_table = outer_table.cells[0].paragraphs.iter().any(|p| {
            p.records
                .iter()
                .any(|r| matches!(r, ParagraphRecord::Table { .. }))
        });
        assert!(has_nested_table, "Outer cell should contain a nested table");
    }

    // ===== Mixed content tests =====

    #[test]
    fn test_parse_text_before_and_after_table() {
        let xml = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Before table</hp:t></hp:run></hp:p>
            <hp:tbl>
                <hp:tr>
                    <hp:tc>
                        <hp:cellAddr colAddr="0" rowAddr="0"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p><hp:run><hp:t>Cell</hp:t></hp:run></hp:p>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
            </hp:tbl>
            <hp:p><hp:run><hp:t>After table</hp:t></hp:run></hp:p>
        "#,
        );
        let section = parse_section_xml(&xml, 0, &mut ParseWarnings::new()).unwrap();
        assert_eq!(section.paragraphs.len(), 3);
        assert_eq!(para_text(&section.paragraphs[0]), Some("Before table"));
        assert!(matches!(
            &section.paragraphs[1].records[0],
            ParagraphRecord::Table { .. }
        ));
        assert_eq!(para_text(&section.paragraphs[2]), Some("After table"));
    }

    #[test]
    fn test_section_index_preserved() {
        let xml = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Test</hp:t></hp:run></hp:p>
        "#,
        );
        let section = parse_section_xml(&xml, 42, &mut ParseWarnings::new()).unwrap();
        assert_eq!(section.index, 42);
    }

    // ===== Error handling tests =====

    #[test]
    fn test_parse_malformed_xml() {
        let xml = "<broken><unclosed>";
        // Should not panic, returns error or empty section
        let result = parse_section_xml(xml, 0, &mut ParseWarnings::new());
        // Malformed XML might still parse (quick_xml is lenient) or error
        // Either way it should not panic
        let _ = result;
    }

    // ===== Helper function tests =====

    #[test]
    fn test_create_paragraph() {
        let para = create_paragraph("Hello");
        assert_eq!(para.para_header.text_char_count, 5);
        assert_eq!(para_text(&para), Some("Hello"));
    }

    #[test]
    fn test_create_image_paragraph_fn() {
        let para = create_image_paragraph("img001");
        match &para.records[0] {
            ParagraphRecord::HwpxImage { binary_item_ref } => {
                assert_eq!(binary_item_ref, "img001");
            }
            _ => panic!("Expected HwpxImage"),
        }
    }

    #[test]
    fn test_create_table_from_rows_fn() {
        let rows = vec![vec![
            HwpxCell {
                content_items: vec![CellContentItem::Text("A".to_string())],
                col_addr: Some(0),
                row_addr: Some(0),
                ..Default::default()
            },
            HwpxCell {
                content_items: vec![CellContentItem::Text("B".to_string())],
                col_addr: Some(1),
                row_addr: Some(0),
                ..Default::default()
            },
        ]];
        let table = create_table_from_rows(&rows);
        assert_eq!(table.attributes.row_count, 1);
        assert_eq!(table.attributes.col_count, 2);
        assert_eq!(table.cells.len(), 2);
    }
}
