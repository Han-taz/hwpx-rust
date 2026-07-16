/// HWPX section XML parser
///
/// Section files (section0.xml, section1.xml, etc.) contain the main document content
/// including paragraphs, tables, images, and other elements.
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesCData, BytesStart, BytesText, Event};
use quick_xml::Reader;

use crate::diagnostics::{
    DiagnosticCategory, DiagnosticContext, DiagnosticItem, DiagnosticReport, DiagnosticSeverity,
};
use crate::document::bodytext::line_seg::LineSegmentTag;
use crate::document::bodytext::list_header::{
    LineBreak, ListHeader, ListHeaderAttribute, TextDirection, VerticalAlign,
};
use crate::document::bodytext::para_header::ParaHeader;
use crate::document::bodytext::table::{
    CellAttributes, PageBreakBehavior, Table, TableAttribute, TableAttributes, TableCell,
    TablePadding,
};
use crate::document::bodytext::{
    LineSegmentInfo, ParaTextRun, Paragraph, ParagraphRecord, Section,
};
use crate::document::BodyText;
use crate::error::{HwpError, ParseWarning, ParseWarnings};
use crate::types::{HWPUNIT, UINT16, WORD};

use super::bindata::normalize_hwpx_binary_item_ref;
use super::container::HwpxContainer;
use super::package::package_section_file_entries;
use super::xml_attr::{
    for_each_xml_attribute, parse_numeric_attr, parse_string_attr, XmlAttributeValueError,
};
use super::xml_budget::XmlParseBudget;

pub(crate) const MAX_HWPX_SECTION_XML_SIZE: u64 = 96 * 1024 * 1024;
pub(crate) const MAX_HWPX_SECTION_PARAGRAPHS: u64 = 200_000;
pub(crate) const MAX_HWPX_SECTION_TEXT_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_HWPX_SECTION_TEXT_RUNS_PER_PARAGRAPH: u64 = 65_536;
pub(crate) const MAX_HWPX_SECTION_TABLE_ROWS: u64 = u16::MAX as u64;
pub(crate) const MAX_HWPX_SECTION_TABLE_CELLS: u64 = 500_000;
pub(crate) const MAX_HWPX_TABLE_CELL_PARAGRAPHS: u64 = i16::MAX as u64;

#[derive(Debug, Clone, Copy)]
struct SectionStructureLimits {
    max_paragraphs: u64,
    max_text_bytes: u64,
    max_text_runs_per_paragraph: u64,
    max_table_rows: u64,
    max_table_cells: u64,
}

impl Default for SectionStructureLimits {
    fn default() -> Self {
        Self {
            max_paragraphs: MAX_HWPX_SECTION_PARAGRAPHS,
            max_text_bytes: MAX_HWPX_SECTION_TEXT_BYTES,
            max_text_runs_per_paragraph: MAX_HWPX_SECTION_TEXT_RUNS_PER_PARAGRAPH,
            max_table_rows: MAX_HWPX_SECTION_TABLE_ROWS,
            max_table_cells: MAX_HWPX_SECTION_TABLE_CELLS,
        }
    }
}

struct SectionStructureBudget<'a> {
    source: &'a str,
    limits: SectionStructureLimits,
    paragraph_count: u64,
    text_bytes: u64,
    text_runs_in_current_paragraph: u64,
    table_rows: u64,
    table_cells: u64,
}

impl<'a> SectionStructureBudget<'a> {
    fn new(source: &'a str, limits: SectionStructureLimits) -> Self {
        Self {
            source,
            limits,
            paragraph_count: 0,
            text_bytes: 0,
            text_runs_in_current_paragraph: 0,
            table_rows: 0,
            table_cells: 0,
        }
    }

    fn start_paragraph(&mut self) {
        self.text_runs_in_current_paragraph = 0;
    }

    fn add_text(&mut self, text: &str) -> Result<(), HwpError> {
        self.text_bytes = self.text_bytes.saturating_add(text.len() as u64);
        if self.text_bytes > self.limits.max_text_bytes {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX section text bytes",
                path: self.source.to_string(),
                limit: self.limits.max_text_bytes,
                actual: self.text_bytes,
            });
        }

        Ok(())
    }

    fn add_text_run(&mut self) -> Result<(), HwpError> {
        self.text_runs_in_current_paragraph = self.text_runs_in_current_paragraph.saturating_add(1);
        if self.text_runs_in_current_paragraph > self.limits.max_text_runs_per_paragraph {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX section text run count per paragraph",
                path: self.source.to_string(),
                limit: self.limits.max_text_runs_per_paragraph,
                actual: self.text_runs_in_current_paragraph,
            });
        }

        Ok(())
    }

    fn add_paragraph(&mut self) -> Result<(), HwpError> {
        self.paragraph_count = self.paragraph_count.saturating_add(1);
        if self.paragraph_count > self.limits.max_paragraphs {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX section paragraph count",
                path: self.source.to_string(),
                limit: self.limits.max_paragraphs,
                actual: self.paragraph_count,
            });
        }

        Ok(())
    }

    fn add_table_row(&mut self) -> Result<(), HwpError> {
        self.table_rows = self.table_rows.saturating_add(1);
        if self.table_rows > self.limits.max_table_rows {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX section table row count",
                path: self.source.to_string(),
                limit: self.limits.max_table_rows,
                actual: self.table_rows,
            });
        }

        Ok(())
    }

    fn add_table_cell(&mut self) -> Result<(), HwpError> {
        self.table_cells = self.table_cells.saturating_add(1);
        if self.table_cells > self.limits.max_table_cells {
            return Err(HwpError::ResourceLimitExceeded {
                resource: "HWPX section table cell count",
                path: self.source.to_string(),
                limit: self.limits.max_table_cells,
                actual: self.table_cells,
            });
        }

        Ok(())
    }
}

/// Content item type within a cell paragraph
/// 셀 문단 내 콘텐츠 항목 유형
#[derive(Debug, Clone)]
enum CellContentItem {
    Paragraph(Paragraph),
    Image(HwpxImageInfo),
    NestedTable(Table),
}

#[derive(Debug, Clone, Default)]
struct HwpxImageInfo {
    binary_item_ref: String,
    brightness: Option<i16>,
    contrast: Option<i16>,
    effect: Option<String>,
    alpha: Option<u8>,
}

/// Cell data with colspan/rowspan and address information
#[derive(Debug, Clone)]
struct HwpxCell {
    /// 현재 문단의 텍스트 / Current paragraph text
    current_text: String,
    /// 현재 문단의 스타일별 텍스트 run / Styled text runs for the current paragraph
    current_runs: Vec<TextRunInfo>,
    /// 현재 run에 누적 중인 텍스트 / Text currently accumulated for the active run
    current_run_text: String,
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
    current_cell_para_shape_id: u16,
    current_cell_para_style_id: u8,
    current_cell_para_emitted_control: bool,
    current_cell_line_segments: Vec<LineSegmentInfo>,
    cell_para_depth: u32,
}

impl Default for HwpxCell {
    fn default() -> Self {
        Self {
            current_text: String::new(),
            current_runs: Vec::with_capacity(4),
            current_run_text: String::new(),
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

fn push_text_run(
    budget: &mut SectionStructureBudget<'_>,
    current_runs: &mut Vec<TextRunInfo>,
    text: String,
    char_shape_id: Option<u16>,
) -> Result<(), HwpError> {
    if text.is_empty() {
        return Ok(());
    }

    budget.add_text_run()?;
    current_runs.push(TextRunInfo {
        text,
        char_shape_id,
    });
    Ok(())
}

fn push_current_text_run(
    budget: &mut SectionStructureBudget<'_>,
    current_runs: &mut Vec<TextRunInfo>,
    current_run_text: &mut String,
    char_shape_id: Option<u16>,
) -> Result<(), HwpError> {
    if current_run_text.is_empty() {
        return Ok(());
    }

    push_text_run(
        budget,
        current_runs,
        std::mem::take(current_run_text),
        char_shape_id,
    )
}

fn push_pending_nested_paragraph_break(
    budget: &mut SectionStructureBudget<'_>,
    current_runs: &mut Vec<TextRunInfo>,
    current_run_text: &mut String,
    char_shape_id: Option<u16>,
    pending_break: &mut bool,
) -> Result<(), HwpError> {
    if !*pending_break {
        return Ok(());
    }

    push_current_text_run(budget, current_runs, current_run_text, char_shape_id)?;
    budget.add_text("\n")?;
    push_text_run(budget, current_runs, "\n".to_string(), None)?;
    *pending_break = false;
    Ok(())
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

fn push_completed_hyperlink_run(
    budget: &mut SectionStructureBudget<'_>,
    current_runs: &mut Vec<TextRunInfo>,
    hyperlink_state: HyperlinkState,
) -> Result<(), HwpError> {
    let HyperlinkState {
        active,
        url,
        text,
        char_shape_id,
    } = hyperlink_state;

    if !active {
        return Ok(());
    }

    let run_text = if url.is_empty() {
        text
    } else {
        format!("\x00HYPERLINK:{url}\x00{text}")
    };

    push_text_run(budget, current_runs, run_text, char_shape_id)
}

#[derive(Debug, Clone, Copy)]
struct NumericAttrIssue<'a> {
    source: &'a str,
    section_index: WORD,
    element: &'a str,
    attribute: &'a str,
    message_prefix: &'a str,
}

fn parse_numeric_attr_or_default<T>(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    issue: NumericAttrIssue<'_>,
    default: T,
) -> Result<T, HwpError>
where
    T: Copy + std::str::FromStr,
{
    match parse_numeric_attr(issue.source, issue.element, issue.attribute, attr) {
        Ok(value) => Ok(value),
        Err(XmlAttributeValueError::InvalidValue(value)) => {
            record_invalid_numeric_attr_diagnostic(warnings, diagnostics, issue, &value);
            Ok(default)
        }
        Err(XmlAttributeValueError::XmlParse(err)) => Err(err),
    }
}

fn parse_optional_numeric_attr_with_diagnostics<T>(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    issue: NumericAttrIssue<'_>,
) -> Result<Option<T>, HwpError>
where
    T: std::str::FromStr,
{
    match parse_numeric_attr(issue.source, issue.element, issue.attribute, attr) {
        Ok(value) => Ok(Some(value)),
        Err(XmlAttributeValueError::InvalidValue(value)) => {
            record_invalid_numeric_attr_diagnostic(warnings, diagnostics, issue, &value);
            Ok(None)
        }
        Err(XmlAttributeValueError::XmlParse(err)) => Err(err),
    }
}

fn record_invalid_numeric_attr<T>(
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    issue: NumericAttrIssue<'_>,
    value: &str,
    default: T,
) -> T {
    record_invalid_numeric_attr_diagnostic(warnings, diagnostics, issue, value);
    default
}

fn record_invalid_numeric_attr_diagnostic(
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    issue: NumericAttrIssue<'_>,
    value: &str,
) {
    let message = format!("{}: {value}", issue.message_prefix);
    warnings.push(ParseWarning::warning(message.clone()));
    record_invalid_value(
        diagnostics,
        issue.source,
        issue.section_index,
        issue.element,
        issue.attribute,
        value,
        message,
    );
}

fn parse_cell_span_attr_or_default(
    attr: &Attribute<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    source: &str,
    section_index: WORD,
    attribute: &str,
    message_prefix: &str,
) -> Result<u16, HwpError> {
    let value = parse_numeric_attr_or_default::<u16>(
        attr,
        warnings,
        diagnostics,
        NumericAttrIssue {
            source,
            section_index,
            element: "hp:cellSpan",
            attribute,
            message_prefix,
        },
        1,
    )?;

    if value == 0 {
        return Ok(record_invalid_numeric_attr(
            warnings,
            diagnostics,
            NumericAttrIssue {
                source,
                section_index,
                element: "hp:cellSpan",
                attribute,
                message_prefix,
            },
            "0",
            1,
        ));
    }

    Ok(value)
}

fn parse_paragraph_shape_style_ids(
    source: &str,
    section_index: WORD,
    element: &BytesStart<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<(u16, u8), HwpError> {
    let mut para_shape_id = 0;
    let mut para_style_id = 0;

    for_each_xml_attribute(source, element, |attr| {
        let key = attr.key.as_ref();
        match key {
            b"paraPrIDRef" | b"prIDRef" => {
                let attribute = if key == b"paraPrIDRef" {
                    "paraPrIDRef"
                } else {
                    "prIDRef"
                };
                para_shape_id = parse_numeric_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    NumericAttrIssue {
                        source,
                        section_index,
                        element: "hp:p",
                        attribute,
                        message_prefix: "Invalid paragraph shape reference value",
                    },
                    0,
                )?;
            }
            b"styleIDRef" => {
                para_style_id = parse_numeric_attr_or_default::<u8>(
                    &attr,
                    warnings,
                    diagnostics,
                    NumericAttrIssue {
                        source,
                        section_index,
                        element: "hp:p",
                        attribute: "styleIDRef",
                        message_prefix: "Invalid paragraph styleIDRef value",
                    },
                    0,
                )?;
            }
            _ => {}
        }
        Ok(())
    })?;

    Ok((para_shape_id, para_style_id))
}

fn parse_hwpx_line_segment(
    source: &str,
    section_index: WORD,
    element: &BytesStart<'_>,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<LineSegmentInfo, HwpError> {
    let mut text_start_position = 0;
    let mut vertical_position = 0;
    let mut line_height = 0;
    let mut text_height = 0;
    let mut baseline_distance = 0;
    let mut line_spacing = 0;
    let mut column_start_position = 0;
    let mut segment_width = 0;
    let mut tag_value = 0;

    for_each_xml_attribute(source, element, |attr| {
        let key = attr.key.as_ref();
        match key {
            b"textpos" => {
                text_start_position = parse_numeric_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    NumericAttrIssue {
                        source,
                        section_index,
                        element: "hp:lineseg",
                        attribute: "textpos",
                        message_prefix: "Invalid line segment textpos value",
                    },
                    0,
                )?;
            }
            b"vertpos" => {
                vertical_position = parse_numeric_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    NumericAttrIssue {
                        source,
                        section_index,
                        element: "hp:lineseg",
                        attribute: "vertpos",
                        message_prefix: "Invalid line segment vertpos value",
                    },
                    0,
                )?;
            }
            b"vertsize" => {
                line_height = parse_numeric_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    NumericAttrIssue {
                        source,
                        section_index,
                        element: "hp:lineseg",
                        attribute: "vertsize",
                        message_prefix: "Invalid line segment vertsize value",
                    },
                    0,
                )?;
            }
            b"textheight" => {
                text_height = parse_numeric_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    NumericAttrIssue {
                        source,
                        section_index,
                        element: "hp:lineseg",
                        attribute: "textheight",
                        message_prefix: "Invalid line segment textheight value",
                    },
                    0,
                )?;
            }
            b"baseline" => {
                baseline_distance = parse_numeric_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    NumericAttrIssue {
                        source,
                        section_index,
                        element: "hp:lineseg",
                        attribute: "baseline",
                        message_prefix: "Invalid line segment baseline value",
                    },
                    0,
                )?;
            }
            b"spacing" => {
                line_spacing = parse_numeric_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    NumericAttrIssue {
                        source,
                        section_index,
                        element: "hp:lineseg",
                        attribute: "spacing",
                        message_prefix: "Invalid line segment spacing value",
                    },
                    0,
                )?;
            }
            b"horzpos" => {
                column_start_position = parse_numeric_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    NumericAttrIssue {
                        source,
                        section_index,
                        element: "hp:lineseg",
                        attribute: "horzpos",
                        message_prefix: "Invalid line segment horzpos value",
                    },
                    0,
                )?;
            }
            b"horzsize" => {
                segment_width = parse_numeric_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    NumericAttrIssue {
                        source,
                        section_index,
                        element: "hp:lineseg",
                        attribute: "horzsize",
                        message_prefix: "Invalid line segment horzsize value",
                    },
                    0,
                )?;
            }
            b"flags" => {
                tag_value = parse_numeric_attr_or_default(
                    &attr,
                    warnings,
                    diagnostics,
                    NumericAttrIssue {
                        source,
                        section_index,
                        element: "hp:lineseg",
                        attribute: "flags",
                        message_prefix: "Invalid line segment flags value",
                    },
                    0,
                )?;
            }
            _ => {}
        }
        Ok(())
    })?;

    Ok(LineSegmentInfo {
        text_start_position,
        vertical_position,
        line_height,
        text_height,
        baseline_distance,
        line_spacing,
        column_start_position,
        segment_width,
        tag: LineSegmentTag::from_bits(tag_value),
    })
}

fn append_line_segments_record(
    paragraph: &mut Paragraph,
    line_segments: &mut Vec<LineSegmentInfo>,
) {
    if line_segments.is_empty() {
        return;
    }

    paragraph.records.push(ParagraphRecord::ParaLineSeg {
        segments: std::mem::take(line_segments),
    });
}

/// Parse all section files and create BodyText
pub fn parse_sections(
    container: &mut HwpxContainer,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<BodyText, HwpError> {
    let section_files = package_section_file_entries(container)?
        .unwrap_or_else(|| container.get_section_file_entries());

    if section_files.is_empty() {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "No section files found in Contents/".to_string(),
        });
    }

    validate_unique_section_indices(&section_files)?;

    let mut sections = Vec::with_capacity(4);

    for (index, section_path) in &section_files {
        let content = container.read_file_string_with_limit(
            section_path,
            MAX_HWPX_SECTION_XML_SIZE,
            "HWPX section XML byte size",
        )?;
        let index = WORD::try_from(*index).map_err(|_| HwpError::InvalidHwpxStructure {
            reason: format!("Section index exceeds WORD range: {section_path}"),
        })?;
        let section = parse_section_xml(&content, index, warnings, diagnostics)?;
        sections.push(section);
    }

    Ok(BodyText { sections })
}

fn validate_unique_section_indices(section_files: &[(usize, String)]) -> Result<(), HwpError> {
    let mut seen = std::collections::BTreeMap::new();

    for (index, path) in section_files {
        if let Some(previous_path) = seen.insert(*index, path) {
            return Err(HwpError::InvalidHwpxStructure {
                reason: format!(
                    "Duplicate section index {index} in HWPX archive: {previous_path} and {path}",
                ),
            });
        }
    }

    Ok(())
}

/// Parse a single section XML file
fn parse_section_xml(
    content: &str,
    index: WORD,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<Section, HwpError> {
    parse_section_xml_with_limits(
        content,
        index,
        warnings,
        diagnostics,
        SectionStructureLimits::default(),
    )
}

fn parse_section_xml_with_limits(
    content: &str,
    index: WORD,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    limits: SectionStructureLimits,
) -> Result<Section, HwpError> {
    let mut reader = Reader::from_str(content);
    let section_source = format!("Contents/section{index}.xml");

    let mut paragraphs = Vec::with_capacity(16);
    let mut current_text = String::new();
    let mut nested_paragraph_text_seen: Vec<bool> = Vec::new();
    let mut pending_nested_paragraph_break = false;
    let mut in_text = false;
    let mut preserve_text_space = false;
    let mut in_cell = false;
    let mut in_caption = false;
    let mut _in_picture = false;

    // Image parsing
    let mut current_image: Option<HwpxImageInfo> = None;

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
    let mut current_para_emitted_control = false;
    let mut current_line_segments: Vec<LineSegmentInfo> = Vec::new();
    let mut current_cell_para_shape_id: u16 = 0;
    let mut current_cell_para_style_id: u8 = 0;
    let mut current_cell_para_emitted_control = false;
    let mut current_cell_line_segments: Vec<LineSegmentInfo> = Vec::new();
    let mut cell_para_depth: u32 = 0;

    // Track nesting depth for paragraphs and tables
    // 문단과 테이블의 중첩 깊이 추적
    let mut para_depth: u32 = 0;
    let mut table_depth: u32 = 0;

    // Stack to save parent table state when entering nested table
    // 중첩 테이블에 진입할 때 부모 테이블 상태를 저장하는 스택
    let mut table_state_stack: Vec<TableState> = Vec::new();
    let mut table_char_shape_stack: Vec<Option<u16>> = Vec::new();
    let mut unsupported_element_counts: std::collections::BTreeMap<&'static str, usize> =
        std::collections::BTreeMap::new();
    let mut xml_budget = XmlParseBudget::new(&section_source);
    let mut structure_budget = SectionStructureBudget::new(&section_source, limits);
    let mut xml_depth = 0usize;
    let mut section_root_seen = false;

    loop {
        let event = reader.read_event();
        if let Ok(ref event) = event {
            xml_budget.observe_event(event)?;
        }

        match event {
            Ok(Event::Empty(ref e)) => {
                // Handle self-closing tags like <hp:cellSpan ... />, <hp:cellAddr ... />, <hp:tab ... />
                let local_name = e.name();
                let local_name = local_name.as_ref();
                validate_section_root_element(local_name, xml_depth, &mut section_root_seen)?;

                if local_name.ends_with(b":p") || local_name == b"p" {
                    if table_depth == 0 && para_depth == 0 {
                        structure_budget.start_paragraph();
                        let (para_shape_id, para_style_id) = parse_paragraph_shape_style_ids(
                            &section_source,
                            index,
                            e,
                            warnings,
                            diagnostics,
                        )?;
                        let mut para = create_paragraph("", &section_source)?;
                        para.para_header.para_shape_id = para_shape_id;
                        para.para_header.para_style_id = para_style_id;
                        push_section_paragraph(&mut structure_budget, &mut paragraphs, para)?;
                    } else if table_depth > 0 && in_cell && cell_para_depth == 0 {
                        structure_budget.start_paragraph();
                        let (para_shape_id, para_style_id) = parse_paragraph_shape_style_ids(
                            &section_source,
                            index,
                            e,
                            warnings,
                            diagnostics,
                        )?;
                        let mut para = create_paragraph("", &section_source)?;
                        para.para_header.para_shape_id = para_shape_id;
                        para.para_header.para_style_id = para_style_id;
                        current_cell
                            .content_items
                            .push(CellContentItem::Paragraph(para));
                    }
                } else if local_name.ends_with(b":lineseg") || local_name == b"lineseg" {
                    let segment =
                        parse_hwpx_line_segment(&section_source, index, e, warnings, diagnostics)?;
                    if table_depth > 0 && in_cell && cell_para_depth > 0 {
                        current_cell_line_segments.push(segment);
                    } else if table_depth == 0 && para_depth == 1 {
                        current_line_segments.push(segment);
                    }
                } else if local_name.ends_with(b":tab") || local_name == b"tab" {
                    // Parse tab element and convert to appropriate text representation
                    // Tab attributes: width (HWPUNIT), leader (0=none, 1=solid, 2=dash, 3=dot), type
                    let mut leader: u8 = 0;
                    let mut width: u32 = 0;

                    for_each_xml_attribute(&section_source, e, |attr| {
                        let key = attr.key.as_ref();
                        match key {
                            b"leader" => {
                                leader = parse_numeric_attr_or_default(
                                    &attr,
                                    warnings,
                                    diagnostics,
                                    NumericAttrIssue {
                                        source: &section_source,
                                        section_index: index,
                                        element: "hp:tab",
                                        attribute: "leader",
                                        message_prefix: "Invalid tab leader value",
                                    },
                                    0,
                                )?;
                            }
                            b"width" => {
                                width = parse_numeric_attr_or_default(
                                    &attr,
                                    warnings,
                                    diagnostics,
                                    NumericAttrIssue {
                                        source: &section_source,
                                        section_index: index,
                                        element: "hp:tab",
                                        attribute: "width",
                                        message_prefix: "Invalid tab width value",
                                    },
                                    0,
                                )?;
                            }
                            _ => {}
                        }
                        Ok(())
                    })?;

                    // Generate tab representation based on leader type
                    // Leader: 0=none, 1=solid, 2=dash, 3=dot
                    let tab_text = match leader {
                        3 => {
                            // Dot leader - generate dots based on approximate width
                            // HWPUNIT: 7200 units = 1 inch, roughly 6 chars per inch
                            let dot_count = (width / 1200).clamp(3, 80) as usize;
                            ".".repeat(dot_count)
                        }
                        2 => {
                            // Dash leader
                            let dash_count = (width / 2400).clamp(2, 40) as usize;
                            "-".repeat(dash_count)
                        }
                        1 => {
                            // Solid line leader
                            let line_count = (width / 2400).clamp(2, 40) as usize;
                            "_".repeat(line_count)
                        }
                        _ => {
                            // No leader - use tab character or spaces
                            "\t".to_string()
                        }
                    };

                    // Add tab representation to current text context
                    let in_table = table_depth > 0;
                    if !in_table {
                        push_pending_nested_paragraph_break(
                            &mut structure_budget,
                            &mut current_runs,
                            &mut current_run_text,
                            current_char_shape_id,
                            &mut pending_nested_paragraph_break,
                        )?;
                    }
                    structure_budget.add_text(&tab_text)?;
                    if in_table && in_caption {
                        table_caption.push_str(&tab_text);
                    } else if in_table && in_cell {
                        current_cell.current_text.push_str(&tab_text);
                        if hyperlink_state.active {
                            hyperlink_state.text.push_str(&tab_text);
                        } else {
                            current_cell.current_run_text.push_str(&tab_text);
                        }
                    } else if !in_table {
                        if para_depth > 1 {
                            if let Some(text_seen) = nested_paragraph_text_seen.last_mut() {
                                *text_seen = true;
                            }
                        }
                        if hyperlink_state.active {
                            hyperlink_state.text.push_str(&tab_text);
                        } else {
                            current_run_text.push_str(&tab_text);
                        }
                    }
                } else if local_name.ends_with(b":cellSpan") || local_name == b"cellSpan" {
                    // Parse colspan and rowspan attributes
                    for_each_xml_attribute(&section_source, e, |attr| {
                        let key = attr.key.as_ref();
                        match key {
                            b"colSpan" => {
                                current_cell.col_span = parse_cell_span_attr_or_default(
                                    &attr,
                                    warnings,
                                    diagnostics,
                                    &section_source,
                                    index,
                                    "colSpan",
                                    "Invalid cellSpan colSpan value",
                                )?;
                            }
                            b"rowSpan" => {
                                current_cell.row_span = parse_cell_span_attr_or_default(
                                    &attr,
                                    warnings,
                                    diagnostics,
                                    &section_source,
                                    index,
                                    "rowSpan",
                                    "Invalid cellSpan rowSpan value",
                                )?;
                            }
                            _ => {}
                        }
                        Ok(())
                    })?;
                } else if local_name.ends_with(b":cellAddr") || local_name == b"cellAddr" {
                    // Parse cell address (actual column and row position)
                    for_each_xml_attribute(&section_source, e, |attr| {
                        let key = attr.key.as_ref();
                        match key {
                            b"colAddr" => {
                                current_cell.col_addr = Some(parse_numeric_attr_or_default(
                                    &attr,
                                    warnings,
                                    diagnostics,
                                    NumericAttrIssue {
                                        source: &section_source,
                                        section_index: index,
                                        element: "hp:cellAddr",
                                        attribute: "colAddr",
                                        message_prefix: "Invalid cellAddr colAddr value",
                                    },
                                    0,
                                )?);
                            }
                            b"rowAddr" => {
                                current_cell.row_addr = Some(parse_numeric_attr_or_default(
                                    &attr,
                                    warnings,
                                    diagnostics,
                                    NumericAttrIssue {
                                        source: &section_source,
                                        section_index: index,
                                        element: "hp:cellAddr",
                                        attribute: "rowAddr",
                                        message_prefix: "Invalid cellAddr rowAddr value",
                                    },
                                    0,
                                )?);
                            }
                            _ => {}
                        }
                        Ok(())
                    })?;
                } else if local_name.ends_with(b":img") || local_name == b"img" {
                    // Parse image element - extract binaryItemIDRef and rendering attributes.
                    // <hc:img binaryItemIDRef="image1" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/>
                    let mut image = current_image.take().unwrap_or_default();
                    let mut unsafe_image_ref = false;
                    for_each_xml_attribute(&section_source, e, |attr| {
                        let key = attr.key.as_ref();
                        match key {
                            b"binaryItemIDRef" => {
                                let value = parse_string_attr(
                                    &section_source,
                                    "hc:img",
                                    "binaryItemIDRef",
                                    &attr,
                                )?;
                                match normalize_hwpx_binary_item_ref(&value) {
                                    Some(image_ref) => image.binary_item_ref = image_ref,
                                    None => {
                                        unsafe_image_ref = true;
                                        record_skipped_image_ref(
                                            warnings,
                                            diagnostics,
                                            &section_source,
                                            index,
                                            &value,
                                        );
                                    }
                                }
                            }
                            b"bright" => {
                                image.brightness = parse_optional_numeric_attr_with_diagnostics(
                                    &attr,
                                    warnings,
                                    diagnostics,
                                    NumericAttrIssue {
                                        source: &section_source,
                                        section_index: index,
                                        element: "hc:img",
                                        attribute: "bright",
                                        message_prefix: "Invalid image bright value",
                                    },
                                )?;
                            }
                            b"contrast" => {
                                image.contrast = parse_optional_numeric_attr_with_diagnostics(
                                    &attr,
                                    warnings,
                                    diagnostics,
                                    NumericAttrIssue {
                                        source: &section_source,
                                        section_index: index,
                                        element: "hc:img",
                                        attribute: "contrast",
                                        message_prefix: "Invalid image contrast value",
                                    },
                                )?;
                            }
                            b"effect" => {
                                image.effect = Some(parse_string_attr(
                                    &section_source,
                                    "hc:img",
                                    "effect",
                                    &attr,
                                )?);
                            }
                            b"alpha" => {
                                image.alpha = parse_optional_numeric_attr_with_diagnostics(
                                    &attr,
                                    warnings,
                                    diagnostics,
                                    NumericAttrIssue {
                                        source: &section_source,
                                        section_index: index,
                                        element: "hc:img",
                                        attribute: "alpha",
                                        message_prefix: "Invalid image alpha value",
                                    },
                                )?;
                            }
                            _ => {}
                        }
                        Ok(())
                    })?;
                    current_image = if unsafe_image_ref || image.binary_item_ref.is_empty() {
                        None
                    } else {
                        Some(image)
                    };
                } else if local_name.ends_with(b":fieldEnd") || local_name == b"fieldEnd" {
                    // Handle self-closing fieldEnd: <hp:fieldEnd beginIDRef="..." />
                    // Self-closing fieldEnd 처리: <hp:fieldEnd beginIDRef="..." />
                    if hyperlink_state.active {
                        let completed_hyperlink = std::mem::take(&mut hyperlink_state);
                        if table_depth > 0 && in_cell {
                            push_completed_hyperlink_run(
                                &mut structure_budget,
                                &mut current_cell.current_runs,
                                completed_hyperlink,
                            )?;
                        } else {
                            push_completed_hyperlink_run(
                                &mut structure_budget,
                                &mut current_runs,
                                completed_hyperlink,
                            )?;
                        }
                    }
                }
                if let Some(element) = unsupported_section_element_name(local_name) {
                    *unsupported_element_counts.entry(element).or_insert(0) += 1;
                }
            }
            Ok(Event::Start(ref e)) => {
                let local_name = e.name();
                let local_name = local_name.as_ref();
                validate_section_root_element(local_name, xml_depth, &mut section_root_seen)?;
                xml_depth = xml_depth.saturating_add(1);

                match local_name {
                    s if s.ends_with(b":p") || s == b"p" => {
                        para_depth += 1;
                        if table_depth == 0 && para_depth > 1 {
                            push_pending_nested_paragraph_break(
                                &mut structure_budget,
                                &mut current_runs,
                                &mut current_run_text,
                                current_char_shape_id,
                                &mut pending_nested_paragraph_break,
                            )?;
                            nested_paragraph_text_seen.push(false);
                        }
                        if table_depth == 0 && para_depth == 1 {
                            current_text.clear();
                            nested_paragraph_text_seen.clear();
                            pending_nested_paragraph_break = false;
                            current_runs.clear();
                            current_run_text.clear();
                            current_line_segments.clear();
                            structure_budget.start_paragraph();
                            // Parse prIDRef and styleIDRef from <hp:p>
                            // <hp:p> 요소에서 prIDRef, styleIDRef 파싱
                            current_para_emitted_control = false;
                            (current_para_shape_id, current_para_style_id) =
                                parse_paragraph_shape_style_ids(
                                    &section_source,
                                    index,
                                    e,
                                    warnings,
                                    diagnostics,
                                )?;
                        } else if table_depth > 0 && in_cell {
                            if cell_para_depth == 0 {
                                structure_budget.start_paragraph();
                                current_cell.current_text.clear();
                                current_cell.current_runs.clear();
                                current_cell.current_run_text.clear();
                                current_cell_line_segments.clear();
                                current_cell_para_emitted_control = false;
                                (current_cell_para_shape_id, current_cell_para_style_id) =
                                    parse_paragraph_shape_style_ids(
                                        &section_source,
                                        index,
                                        e,
                                        warnings,
                                        diagnostics,
                                    )?;
                            }
                            cell_para_depth = cell_para_depth.saturating_add(1);
                        }
                    }
                    s if s.ends_with(b":run") || s == b"run" => {
                        // Save previous run if any text accumulated
                        // 이전 run의 텍스트가 있으면 저장
                        if table_depth > 0 && in_cell {
                            push_current_text_run(
                                &mut structure_budget,
                                &mut current_cell.current_runs,
                                &mut current_cell.current_run_text,
                                current_char_shape_id,
                            )?;
                        } else {
                            push_current_text_run(
                                &mut structure_budget,
                                &mut current_runs,
                                &mut current_run_text,
                                current_char_shape_id,
                            )?;
                        }
                        // Parse charPrIDRef from <hp:run charPrIDRef="N">
                        current_char_shape_id = None;
                        for_each_xml_attribute(&section_source, e, |attr| {
                            let key = attr.key.as_ref();
                            if key == b"charPrIDRef" {
                                current_char_shape_id =
                                    parse_optional_numeric_attr_with_diagnostics::<u16>(
                                        &attr,
                                        warnings,
                                        diagnostics,
                                        NumericAttrIssue {
                                            source: &section_source,
                                            section_index: index,
                                            element: "hp:run",
                                            attribute: "charPrIDRef",
                                            message_prefix: "Invalid run charPrIDRef value",
                                        },
                                    )?;
                            }
                            Ok(())
                        })?;
                    }
                    s if s.ends_with(b":t") || s == b"t" => {
                        in_text = true;
                        preserve_text_space = false;
                        for_each_xml_attribute(&section_source, e, |attr| {
                            if attr.key.as_ref() == b"xml:space" {
                                let value =
                                    parse_string_attr(&section_source, "hp:t", "xml:space", &attr)?;
                                preserve_text_space = value == "preserve";
                            }
                            Ok(())
                        })?;
                    }
                    s if s.ends_with(b":tbl") || s == b"tbl" => {
                        table_char_shape_stack.push(current_char_shape_id);
                        // If already in a table (nested table), save current state
                        // 이미 테이블 안에 있으면 (중첩 테이블) 현재 상태 저장
                        if table_depth > 0 {
                            table_state_stack.push(TableState {
                                table_rows: std::mem::take(&mut table_rows),
                                current_row: std::mem::take(&mut current_row),
                                current_cell: std::mem::take(&mut current_cell),
                                table_caption: std::mem::take(&mut table_caption),
                                in_cell,
                                current_cell_para_shape_id,
                                current_cell_para_style_id,
                                current_cell_para_emitted_control,
                                current_cell_line_segments: std::mem::take(
                                    &mut current_cell_line_segments,
                                ),
                                cell_para_depth,
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
                        current_cell_para_shape_id = 0;
                        current_cell_para_style_id = 0;
                        current_cell_para_emitted_control = false;
                        current_cell_line_segments.clear();
                        cell_para_depth = 0;
                    }
                    s if s.ends_with(b":lineseg") || s == b"lineseg" => {
                        let segment = parse_hwpx_line_segment(
                            &section_source,
                            index,
                            e,
                            warnings,
                            diagnostics,
                        )?;
                        if table_depth > 0 && in_cell && cell_para_depth > 0 {
                            current_cell_line_segments.push(segment);
                        } else if table_depth == 0 && para_depth == 1 {
                            current_line_segments.push(segment);
                        }
                    }
                    s if s.ends_with(b":pic") || s == b"pic" => {
                        _in_picture = true;
                        current_image = None;
                    }
                    s if s.ends_with(b":fieldBegin") || s == b"fieldBegin" => {
                        // Parse fieldBegin for hyperlinks
                        // <hp:fieldBegin type="HYPERLINK" id="...">
                        in_field_begin = true;
                        let mut is_hyperlink = false;
                        let mut field_type: Option<String> = None;

                        for_each_xml_attribute(&section_source, e, |attr| {
                            let key = attr.key.as_ref();
                            if key == b"type" {
                                let value = parse_string_attr(
                                    &section_source,
                                    "hp:fieldBegin",
                                    "type",
                                    &attr,
                                )?;
                                is_hyperlink = value == "HYPERLINK";
                                field_type = Some(value);
                            }
                            Ok(())
                        })?;

                        if let Some(value) = field_type {
                            if value != "HYPERLINK" {
                                diagnostics.push(
                                    DiagnosticItem::new(
                                        DiagnosticSeverity::Unsupported,
                                        DiagnosticCategory::UnsupportedAttribute,
                                        format!("Unsupported HWPX fieldBegin type {value}"),
                                    )
                                    .with_context(
                                        DiagnosticContext::new()
                                            .with_source(section_source.as_str())
                                            .with_section_index(index)
                                            .with_element("hp:fieldBegin")
                                            .with_attribute("type")
                                            .with_value(value)
                                            .with_component("hwpx::section"),
                                    ),
                                );
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
                    s if (s.ends_with(b":stringParam") || s == b"stringParam")
                        && in_parameters
                        && hyperlink_state.active =>
                    {
                        // Parse stringParam for URL extraction
                        // <hp:stringParam name="Path">URL</hp:stringParam>
                        for_each_xml_attribute(&section_source, e, |attr| {
                            let key = attr.key.as_ref();
                            if key == b"name" {
                                current_param_name = parse_string_attr(
                                    &section_source,
                                    "hp:stringParam",
                                    "name",
                                    &attr,
                                )?;
                            }
                            Ok(())
                        })?;
                    }
                    _ => {}
                }
                if let Some(element) = unsupported_section_element_name(local_name) {
                    *unsupported_element_counts.entry(element).or_insert(0) += 1;
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = normalize_section_text(
                    unescape_section_text(&section_source, e)?,
                    preserve_text_space,
                );

                // Handle stringParam content for hyperlink URL
                if in_parameters && hyperlink_state.active && current_param_name == "Path" {
                    structure_budget.add_text(&text)?;
                    hyperlink_state.url.push_str(&text);
                } else if in_text {
                    let in_table = table_depth > 0;
                    if in_table && in_caption {
                        structure_budget.add_text(&text)?;
                        // Text inside table caption
                        table_caption.push_str(&text);
                    } else if in_table && in_cell {
                        structure_budget.add_text(&text)?;
                        current_cell.current_text.push_str(&text);
                        if hyperlink_state.active {
                            hyperlink_state.text.push_str(&text);
                            if hyperlink_state.char_shape_id.is_none() {
                                hyperlink_state.char_shape_id = current_char_shape_id;
                            }
                        } else {
                            current_cell.current_run_text.push_str(&text);
                        }
                    } else if !in_table {
                        if !text.is_empty() {
                            push_pending_nested_paragraph_break(
                                &mut structure_budget,
                                &mut current_runs,
                                &mut current_run_text,
                                current_char_shape_id,
                                &mut pending_nested_paragraph_break,
                            )?;
                            if para_depth > 1 {
                                if let Some(text_seen) = nested_paragraph_text_seen.last_mut() {
                                    *text_seen = true;
                                }
                            }
                        }
                        structure_budget.add_text(&text)?;
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
                        }
                    }
                }
            }
            Ok(Event::CData(ref e)) => {
                let text = normalize_section_text(
                    decode_section_cdata(&section_source, e)?,
                    preserve_text_space,
                );

                if in_parameters && hyperlink_state.active && current_param_name == "Path" {
                    structure_budget.add_text(&text)?;
                    hyperlink_state.url.push_str(&text);
                } else if in_text {
                    let in_table = table_depth > 0;
                    if in_table && in_caption {
                        structure_budget.add_text(&text)?;
                        table_caption.push_str(&text);
                    } else if in_table && in_cell {
                        structure_budget.add_text(&text)?;
                        current_cell.current_text.push_str(&text);
                        if hyperlink_state.active {
                            hyperlink_state.text.push_str(&text);
                            if hyperlink_state.char_shape_id.is_none() {
                                hyperlink_state.char_shape_id = current_char_shape_id;
                            }
                        } else {
                            current_cell.current_run_text.push_str(&text);
                        }
                    } else if !in_table {
                        if !text.is_empty() {
                            push_pending_nested_paragraph_break(
                                &mut structure_budget,
                                &mut current_runs,
                                &mut current_run_text,
                                current_char_shape_id,
                                &mut pending_nested_paragraph_break,
                            )?;
                            if para_depth > 1 {
                                if let Some(text_seen) = nested_paragraph_text_seen.last_mut() {
                                    *text_seen = true;
                                }
                            }
                        }
                        structure_budget.add_text(&text)?;
                        if hyperlink_state.active {
                            hyperlink_state.text.push_str(&text);
                            if hyperlink_state.char_shape_id.is_none() {
                                hyperlink_state.char_shape_id = current_char_shape_id;
                            }
                        } else {
                            current_run_text.push_str(&text);
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
                        if table_depth > 0 && in_cell {
                            push_current_text_run(
                                &mut structure_budget,
                                &mut current_cell.current_runs,
                                &mut current_cell.current_run_text,
                                current_char_shape_id,
                            )?;
                        } else {
                            push_current_text_run(
                                &mut structure_budget,
                                &mut current_runs,
                                &mut current_run_text,
                                current_char_shape_id,
                            )?;
                        }
                    }
                    s if s.ends_with(b":p") || s == b"p" => {
                        let in_table = table_depth > 0;
                        if para_depth == 1 && !in_table {
                            // Save any remaining run text
                            push_current_text_run(
                                &mut structure_budget,
                                &mut current_runs,
                                &mut current_run_text,
                                current_char_shape_id,
                            )?;
                            // Create paragraph with runs if any
                            if !current_runs.is_empty() {
                                let mut para =
                                    create_paragraph_with_runs(&mut current_runs, &section_source)?;
                                para.para_header.para_shape_id = current_para_shape_id;
                                para.para_header.para_style_id = current_para_style_id;
                                append_line_segments_record(&mut para, &mut current_line_segments);
                                push_section_paragraph(
                                    &mut structure_budget,
                                    &mut paragraphs,
                                    para,
                                )?;
                            } else if !current_text.is_empty() {
                                // Fallback to old behavior
                                let mut para = create_paragraph(&current_text, &section_source)?;
                                para.para_header.para_shape_id = current_para_shape_id;
                                para.para_header.para_style_id = current_para_style_id;
                                append_line_segments_record(&mut para, &mut current_line_segments);
                                push_section_paragraph(
                                    &mut structure_budget,
                                    &mut paragraphs,
                                    para,
                                )?;
                            } else if !current_para_emitted_control
                                || !current_line_segments.is_empty()
                            {
                                let mut para = create_paragraph("", &section_source)?;
                                para.para_header.para_shape_id = current_para_shape_id;
                                para.para_header.para_style_id = current_para_style_id;
                                append_line_segments_record(&mut para, &mut current_line_segments);
                                push_section_paragraph(
                                    &mut structure_budget,
                                    &mut paragraphs,
                                    para,
                                )?;
                            }
                            current_line_segments.clear();
                            current_text.clear();
                            nested_paragraph_text_seen.clear();
                            pending_nested_paragraph_break = false;
                        }
                        // Preserve each table-cell paragraph, including empty paragraphs.
                        if in_cell && in_table && cell_para_depth == 1 {
                            push_current_text_run(
                                &mut structure_budget,
                                &mut current_cell.current_runs,
                                &mut current_cell.current_run_text,
                                current_char_shape_id,
                            )?;
                            let text = std::mem::take(&mut current_cell.current_text);
                            if !current_cell.current_runs.is_empty() {
                                let mut para = create_paragraph_with_runs(
                                    &mut current_cell.current_runs,
                                    &section_source,
                                )?;
                                para.para_header.para_shape_id = current_cell_para_shape_id;
                                para.para_header.para_style_id = current_cell_para_style_id;
                                append_line_segments_record(
                                    &mut para,
                                    &mut current_cell_line_segments,
                                );
                                current_cell
                                    .content_items
                                    .push(CellContentItem::Paragraph(para));
                            } else if !text.is_empty()
                                || !current_cell_para_emitted_control
                                || !current_cell_line_segments.is_empty()
                            {
                                let mut para =
                                    create_paragraph_from_owned_text(text, &section_source)?;
                                para.para_header.para_shape_id = current_cell_para_shape_id;
                                para.para_header.para_style_id = current_cell_para_style_id;
                                append_line_segments_record(
                                    &mut para,
                                    &mut current_cell_line_segments,
                                );
                                current_cell
                                    .content_items
                                    .push(CellContentItem::Paragraph(para));
                            }
                            current_cell_line_segments.clear();
                        }
                        if in_cell && in_table {
                            cell_para_depth = cell_para_depth.saturating_sub(1);
                        }
                        // Add newline between nested paragraphs (e.g., in drawText/container)
                        // This ensures proper line breaks in TOC and other nested structures
                        if para_depth > 1 && !in_table {
                            let text_seen = nested_paragraph_text_seen.pop().unwrap_or(false);
                            if text_seen {
                                if let Some(parent_text_seen) =
                                    nested_paragraph_text_seen.last_mut()
                                {
                                    *parent_text_seen = true;
                                }
                                pending_nested_paragraph_break = true;
                            }
                        }
                        para_depth = para_depth.saturating_sub(1);
                    }
                    s if s.ends_with(b":t") || s == b"t" => {
                        in_text = false;
                        preserve_text_space = false;
                    }
                    s if s.ends_with(b":caption") || s == b"caption" => {
                        in_caption = false;
                    }
                    s if s.ends_with(b":tbl") || s == b"tbl" => {
                        table_depth = table_depth.saturating_sub(1);
                        if let Some(parent_char_shape_id) = table_char_shape_stack.pop() {
                            current_char_shape_id = parent_char_shape_id;
                        }

                        if table_depth == 0 {
                            // Outermost table complete - add to paragraphs
                            // 최외곽 테이블 완료 - paragraph로 추가
                            let caption_trimmed = table_caption.trim();
                            if !caption_trimmed.is_empty() {
                                push_section_paragraph(
                                    &mut structure_budget,
                                    &mut paragraphs,
                                    create_paragraph(caption_trimmed, &section_source)?,
                                )?;
                            }
                            if !table_rows.is_empty() {
                                let paragraph = create_table_paragraph_with_spans(
                                    std::mem::take(&mut table_rows),
                                    &section_source,
                                    index,
                                    diagnostics,
                                )?;
                                push_section_paragraph(
                                    &mut structure_budget,
                                    &mut paragraphs,
                                    paragraph,
                                )?;
                            }
                            table_caption.clear();
                        } else {
                            // Nested table complete - convert to content for parent cell
                            // 중첩 테이블 완료 - 부모 셀의 콘텐츠로 변환
                            let nested_table = if !table_rows.is_empty() {
                                Some(create_table_from_rows(
                                    std::mem::take(&mut table_rows),
                                    &section_source,
                                    index,
                                    diagnostics,
                                )?)
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
                                current_cell_para_shape_id =
                                    parent_state.current_cell_para_shape_id;
                                current_cell_para_style_id =
                                    parent_state.current_cell_para_style_id;
                                current_cell_para_emitted_control =
                                    parent_state.current_cell_para_emitted_control;
                                current_cell_line_segments =
                                    parent_state.current_cell_line_segments;
                                cell_para_depth = parent_state.cell_para_depth;

                                // Add nested table to parent cell's content
                                // 중첩 테이블을 부모 셀의 콘텐츠에 추가
                                if let Some(table) = nested_table {
                                    current_cell
                                        .content_items
                                        .push(CellContentItem::NestedTable(table));
                                    current_cell_para_emitted_control = true;
                                }
                            }
                        }
                    }
                    s if s.ends_with(b":tr") || s == b"tr" => {
                        if !current_row.is_empty() {
                            structure_budget.add_table_row()?;
                            table_rows.push(std::mem::take(&mut current_row));
                        }
                    }
                    s if s.ends_with(b":tc") || s == b"tc" => {
                        // Cell parsing complete, push to current row
                        // 셀 파싱 완료, 현재 행에 추가
                        structure_budget.add_table_cell()?;
                        current_row.push(std::mem::take(&mut current_cell));
                        in_cell = false;
                    }
                    s if s.ends_with(b":pic") || s == b"pic" => {
                        // Create image paragraph when picture element ends
                        // 테이블 셀 내부의 이미지는 셀에 저장하고, 그 외에는 별도 paragraph로 추가
                        // Store images inside table cells, otherwise add as separate paragraph
                        if let Some(image) = std::mem::take(&mut current_image) {
                            let in_table = table_depth > 0;
                            if in_table && in_cell {
                                // 테이블 셀 내부의 이미지는 순서대로 콘텐츠 항목에 추가
                                // Add image to content items in order
                                current_cell
                                    .content_items
                                    .push(CellContentItem::Image(image));
                                current_cell_para_emitted_control = true;
                            } else {
                                // 테이블 밖의 이미지는 별도 paragraph로 추가
                                push_section_paragraph(
                                    &mut structure_budget,
                                    &mut paragraphs,
                                    create_image_paragraph(&image),
                                )?;
                                current_para_emitted_control = true;
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
                        if hyperlink_state.active {
                            let completed_hyperlink = std::mem::take(&mut hyperlink_state);
                            if table_depth > 0 && in_cell {
                                push_completed_hyperlink_run(
                                    &mut structure_budget,
                                    &mut current_cell.current_runs,
                                    completed_hyperlink,
                                )?;
                            } else {
                                push_completed_hyperlink_run(
                                    &mut structure_budget,
                                    &mut current_runs,
                                    completed_hyperlink,
                                )?;
                            }
                        }
                    }
                    s if s.ends_with(b":parameters") || s == b"parameters" => {
                        in_parameters = false;
                    }
                    s if s.ends_with(b":stringParam") || s == b"stringParam" => {
                        current_param_name.clear();
                    }
                    _ => {}
                }
                xml_budget.finish_end_event(e)?;
                xml_depth = xml_depth.saturating_sub(1);
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

    if !section_root_seen {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "HWPX section XML root element must be hs:sec".to_string(),
        });
    }

    for (element, count) in unsupported_element_counts {
        diagnostics.push(
            DiagnosticItem::new(
                DiagnosticSeverity::Unsupported,
                DiagnosticCategory::UnsupportedElement,
                format!("Unsupported HWPX element {element} occurred {count} time(s)"),
            )
            .with_context(
                DiagnosticContext::new()
                    .with_source(section_source.as_str())
                    .with_section_index(index)
                    .with_element(element)
                    .with_component("hwpx::section"),
            ),
        );
    }

    Ok(Section { index, paragraphs })
}

fn validate_section_root_element(
    name: &[u8],
    current_depth: usize,
    section_root_seen: &mut bool,
) -> Result<(), HwpError> {
    if current_depth != 0 {
        return Ok(());
    }

    if *section_root_seen {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "HWPX section XML contains multiple root elements".to_string(),
        });
    }

    if !is_section_root_element(name) {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "HWPX section XML root element must be hs:sec".to_string(),
        });
    }

    *section_root_seen = true;
    Ok(())
}

fn is_section_root_element(name: &[u8]) -> bool {
    name == b"sec" || name.ends_with(b":sec")
}

fn push_section_paragraph(
    budget: &mut SectionStructureBudget<'_>,
    paragraphs: &mut Vec<Paragraph>,
    paragraph: Paragraph,
) -> Result<(), HwpError> {
    budget.add_paragraph()?;
    paragraphs.push(paragraph);
    Ok(())
}

fn record_invalid_value(
    diagnostics: &mut DiagnosticReport,
    source: &str,
    section_index: WORD,
    element: &str,
    attribute: &str,
    value: &str,
    message: impl Into<String>,
) {
    diagnostics.push(
        DiagnosticItem::new(
            DiagnosticSeverity::RecoveredError,
            DiagnosticCategory::InvalidValue,
            message,
        )
        .with_context(
            DiagnosticContext::new()
                .with_source(source)
                .with_section_index(section_index)
                .with_element(element)
                .with_attribute(attribute)
                .with_value(value)
                .with_component("hwpx::section"),
        ),
    );
}

fn record_skipped_image_ref(
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
    source: &str,
    section_index: WORD,
    value: &str,
) {
    let message = format!("Skipped unsafe HWPX image binaryItemIDRef: {value}");
    warnings.push(ParseWarning::recovered_error(message.clone()));
    diagnostics.push(
        DiagnosticItem::new(
            DiagnosticSeverity::DataLoss,
            DiagnosticCategory::SkippedBinary,
            message,
        )
        .with_context(
            DiagnosticContext::new()
                .with_source(source)
                .with_section_index(section_index)
                .with_element("hc:img")
                .with_attribute("binaryItemIDRef")
                .with_value(value)
                .with_component("hwpx::section"),
        ),
    );
}

fn unsupported_section_element_name(name: &[u8]) -> Option<&'static str> {
    match name.rsplit(|byte| *byte == b':').next().unwrap_or(name) {
        b"chart" => Some("chart"),
        b"ole" => Some("ole"),
        b"equation" => Some("equation"),
        b"video" => Some("video"),
        _ => None,
    }
}

fn unescape_section_text(source: &str, text: &BytesText<'_>) -> Result<String, HwpError> {
    text.unescape()
        .map(|value| value.into_owned())
        .map_err(|err| HwpError::XmlParseError(format!("Error unescaping text in {source}: {err}")))
}

fn decode_section_cdata(source: &str, cdata: &BytesCData<'_>) -> Result<String, HwpError> {
    cdata
        .decode()
        .map(|value| value.into_owned())
        .map_err(|err| HwpError::XmlParseError(format!("Error decoding CDATA in {source}: {err}")))
}

fn normalize_section_text(text: String, preserve_space: bool) -> String {
    if preserve_space {
        text
    } else {
        text.trim().to_string()
    }
}

/// Create a paragraph from text content
fn create_paragraph(text: &str, source: &str) -> Result<Paragraph, HwpError> {
    create_paragraph_from_owned_text(text.to_string(), source)
}

fn create_paragraph_from_owned_text(text: String, source: &str) -> Result<Paragraph, HwpError> {
    let para_header = ParaHeader {
        text_char_count: paragraph_text_char_count(text.chars().count(), source)?,
        ..Default::default()
    };

    let records = vec![ParagraphRecord::ParaText {
        data: Box::new(crate::document::bodytext::ParaTextData {
            text,
            runs: Vec::new(),
            control_char_positions: vec![],
            inline_control_params: vec![],
        }),
    }];

    Ok(Paragraph {
        para_header,
        records,
    })
}

fn paragraph_text_char_count(char_count: usize, source: &str) -> Result<u32, HwpError> {
    u32::try_from(char_count).map_err(|_| HwpError::ResourceLimitExceeded {
        resource: "HWPX paragraph text character count",
        path: source.to_string(),
        limit: u32::MAX as u64,
        actual: char_count as u64,
    })
}

fn visible_text_len_from_runs(text_runs: &[TextRunInfo]) -> usize {
    text_runs
        .iter()
        .map(|run_info| {
            if run_info.text.is_empty() {
                return 0;
            }

            if let Some(content) = run_info.text.strip_prefix("\x00HYPERLINK:") {
                return content
                    .find('\x00')
                    .map_or(0, |null_pos| content[null_pos + '\x00'.len_utf8()..].len());
            }

            run_info.text.len()
        })
        .sum()
}

/// Create a paragraph from text runs with char_shape_id
/// char_shape_id가 연결된 텍스트 run들로 paragraph 생성
fn create_paragraph_with_runs(
    text_runs: &mut Vec<TextRunInfo>,
    source: &str,
) -> Result<Paragraph, HwpError> {
    // Build runs, handling hyperlink markers
    let mut runs: Vec<ParaTextRun> = Vec::with_capacity(text_runs.len());
    let mut total_text = String::with_capacity(visible_text_len_from_runs(text_runs));

    for run_info in text_runs.drain(..) {
        let TextRunInfo {
            text,
            char_shape_id,
        } = run_info;

        if text.is_empty() {
            continue;
        }

        // Check for hyperlink marker: \x00HYPERLINK:url\x00text
        if let Some(content) = text.strip_prefix("\x00HYPERLINK:") {
            // Parse hyperlink: \x00HYPERLINK:url\x00text
            if let Some(null_pos) = content.find('\x00') {
                let url = &content[..null_pos];
                let text = &content[null_pos + 1..];
                runs.push(ParaTextRun::Hyperlink {
                    text: text.to_string(),
                    url: url.to_string(),
                    char_shape_id,
                });
                total_text.push_str(text);
            }
        } else {
            total_text.push_str(&text);
            runs.push(ParaTextRun::Text {
                text,
                char_shape_id,
            });
        }
    }

    let para_header = ParaHeader {
        text_char_count: paragraph_text_char_count(total_text.chars().count(), source)?,
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

    Ok(Paragraph {
        para_header,
        records,
    })
}

/// Create a Table struct from rows (used for nested tables)
/// 행 데이터로부터 Table 구조체 생성 (중첩 테이블용)
fn create_table_from_rows(
    rows: Vec<Vec<HwpxCell>>,
    source: &str,
    section_index: WORD,
    diagnostics: &mut DiagnosticReport,
) -> Result<Table, HwpError> {
    let metrics = table_rows_metrics(&rows, source)?;
    if let Some(value) = metrics.invalid_column_extent.as_deref() {
        record_invalid_value(
            diagnostics,
            source,
            section_index,
            "hp:cellSpan",
            "colSpan",
            value,
            format!("Invalid cellSpan colSpan geometry: {value}"),
        );
    }

    let table_attributes = TableAttributes {
        attribute: TableAttribute {
            page_break: PageBreakBehavior::NoBreak,
            header_row_repeat: false,
        },
        row_count: metrics.row_count,
        col_count: metrics.col_count,
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

    let mut cells = Vec::with_capacity(metrics.total_cells);

    for (row_idx, row) in rows.into_iter().enumerate() {
        let mut calc_col_address: u16 = 0;

        for cell_data in row {
            let col_address = cell_data.col_addr.unwrap_or(calc_col_address);
            let row_address = cell_data.row_addr.unwrap_or(row_idx as u16);
            let paragraph_count = table_cell_paragraph_count_for_list_header(&cell_data, source)?;
            let col_span = cell_data.col_span;
            let row_span = cell_data.row_span;

            let mut cell_paragraphs = Vec::with_capacity(paragraph_count as usize);

            for item in cell_data.content_items {
                match item {
                    CellContentItem::Paragraph(paragraph) => {
                        cell_paragraphs.push(paragraph);
                    }
                    CellContentItem::Image(image) => {
                        cell_paragraphs.push(create_image_paragraph(&image));
                    }
                    CellContentItem::NestedTable(nested_table) => {
                        // Create a paragraph containing the nested table
                        // 중첩 테이블을 포함하는 paragraph 생성
                        let para_header = ParaHeader {
                            text_char_count: 1,
                            ..Default::default()
                        };
                        let records = vec![ParagraphRecord::Table {
                            table: nested_table,
                        }];
                        cell_paragraphs.push(Paragraph {
                            para_header,
                            records,
                        });
                    }
                }
            }

            if cell_paragraphs.is_empty() {
                cell_paragraphs.push(create_paragraph("", source)?);
            }

            let cell = TableCell {
                list_header: ListHeader {
                    paragraph_count,
                    attribute: ListHeaderAttribute {
                        text_direction: TextDirection::Horizontal,
                        line_break: LineBreak::Normal,
                        vertical_align: VerticalAlign::Top,
                    },
                },
                cell_attributes: CellAttributes {
                    col_address,
                    row_address,
                    col_span,
                    row_span,
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

            calc_col_address = match col_address.checked_add(col_span) {
                Some(next_col_address) => next_col_address,
                None => {
                    let value = format!("{}+{}", col_address, col_span);
                    record_invalid_value(
                        diagnostics,
                        source,
                        section_index,
                        "hp:cellSpan",
                        "colSpan",
                        &value,
                        format!("Invalid cellSpan colSpan geometry: {value}"),
                    );
                    0
                }
            };
        }
    }

    Ok(Table {
        attributes: table_attributes,
        cells,
    })
}

struct TableRowsMetrics {
    row_count: UINT16,
    col_count: UINT16,
    total_cells: usize,
    invalid_column_extent: Option<String>,
}

fn table_rows_metrics(rows: &[Vec<HwpxCell>], source: &str) -> Result<TableRowsMetrics, HwpError> {
    let row_count = UINT16::try_from(rows.len()).map_err(|_| HwpError::ResourceLimitExceeded {
        resource: "HWPX table row count",
        path: source.to_string(),
        limit: u16::MAX as u64,
        actual: rows.len() as u64,
    })?;

    let mut total_cells = 0usize;
    let mut max_column_extent = 0usize;
    let mut invalid_column_extent = None;

    for row in rows {
        total_cells = total_cells.saturating_add(row.len());
        for cell in row {
            let col_addr = cell.col_addr.unwrap_or(0) as usize;
            let span = cell.col_span as usize;
            let extent = col_addr.saturating_add(span);
            max_column_extent = max_column_extent.max(extent);
            if extent > u16::MAX as usize && invalid_column_extent.is_none() {
                invalid_column_extent = Some(format!("{col_addr}+{span}"));
            }
        }
    }

    let col_count = u16::try_from(max_column_extent).unwrap_or(u16::MAX);

    Ok(TableRowsMetrics {
        row_count,
        col_count,
        total_cells,
        invalid_column_extent,
    })
}

fn table_cell_paragraph_count_for_list_header(
    cell_data: &HwpxCell,
    source: &str,
) -> Result<i16, HwpError> {
    let paragraph_count = cell_data
        .content_items
        .iter()
        .filter(|item| match item {
            CellContentItem::Paragraph(_) => true,
            CellContentItem::Image(_) | CellContentItem::NestedTable(_) => true,
        })
        .count()
        .max(1);
    let paragraph_count_u64 = paragraph_count as u64;

    if paragraph_count_u64 > MAX_HWPX_TABLE_CELL_PARAGRAPHS {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX table cell paragraph count",
            path: source.to_string(),
            limit: MAX_HWPX_TABLE_CELL_PARAGRAPHS,
            actual: paragraph_count_u64,
        });
    }

    Ok(paragraph_count as i16)
}

/// Create a paragraph containing a table with proper colspan/rowspan
/// `create_table_from_rows`로 Table을 생성한 후 Paragraph로 래핑
fn create_table_paragraph_with_spans(
    rows: Vec<Vec<HwpxCell>>,
    source: &str,
    section_index: WORD,
    diagnostics: &mut DiagnosticReport,
) -> Result<Paragraph, HwpError> {
    let table = create_table_from_rows(rows, source, section_index, diagnostics)?;

    let para_header = ParaHeader {
        text_char_count: 1, // Table control character
        ..Default::default()
    };

    let records = vec![ParagraphRecord::Table { table }];

    Ok(Paragraph {
        para_header,
        records,
    })
}

/// Create a paragraph containing an image reference
fn create_image_paragraph(image: &HwpxImageInfo) -> Paragraph {
    let para_header = ParaHeader {
        text_char_count: 1, // Image control character
        ..Default::default()
    };

    let records = vec![ParagraphRecord::HwpxImage {
        binary_item_ref: image.binary_item_ref.clone(),
        brightness: image.brightness,
        contrast: image.contrast,
        effect: image.effect.clone(),
        alpha: image.alpha,
    }];

    Paragraph {
        para_header,
        records,
    }
}

#[cfg(test)]
mod tests {
    use super::super::package::{
        MAX_HWPX_CONTENT_HPF_MANIFEST_ITEMS, MAX_HWPX_CONTENT_HPF_SPINE_ITEMREFS,
    };
    use super::super::xml_budget::{
        validate_xml_event_count, MAX_HWPX_XML_DEPTH, MAX_HWPX_XML_EVENTS,
    };
    use super::*;
    use crate::diagnostics::DiagnosticReport;
    use crate::document::bodytext::ParagraphRecord;
    use std::io::{Cursor, Write};
    use zip::{write::SimpleFileOptions, ZipWriter};

    fn zip_with_files(files: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);

        for (path, data) in files {
            zip.start_file(*path, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }

        zip.finish().unwrap().into_inner()
    }

    fn zip_with_stored_files(files: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        for (path, data) in files {
            zip.start_file(*path, options).unwrap();
            zip.write_all(data).unwrap();
        }

        zip.finish().unwrap().into_inner()
    }

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

    fn parse_test_section(xml: &str) -> Section {
        parse_test_section_at(xml, 0)
    }

    fn parse_test_section_at(xml: &str, index: WORD) -> Section {
        parse_test_section_result(xml, index).unwrap()
    }

    fn parse_test_section_result(xml: &str, index: WORD) -> Result<Section, HwpError> {
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();
        parse_section_xml(xml, index, &mut warnings, &mut diagnostics)
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

    fn para_line_segments(para: &Paragraph) -> Option<&Vec<LineSegmentInfo>> {
        para.records.iter().find_map(|record| match record {
            ParagraphRecord::ParaLineSeg { segments } => Some(segments),
            _ => None,
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
        let section = parse_test_section(&xml);
        assert_eq!(section.paragraphs.len(), 1);
        assert_eq!(para_text(&section.paragraphs[0]), Some("Hello World"));
    }

    #[test]
    fn cdata_text_is_preserved_in_paragraphs() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run>
                    <hp:t><![CDATA[Hello <HWPX> & text]]></hp:t>
                </hp:run>
            </hp:p>
        "#,
        );
        let section = parse_test_section(&xml);

        assert_eq!(section.paragraphs.len(), 1);
        assert_eq!(
            para_text(&section.paragraphs[0]),
            Some("Hello <HWPX> & text")
        );
    }

    #[test]
    fn paragraph_text_trims_edge_whitespace_by_default() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run>
                    <hp:t>  leading and trailing  </hp:t>
                </hp:run>
            </hp:p>
        "#,
        );
        let section = parse_test_section(&xml);

        assert_eq!(section.paragraphs.len(), 1);
        assert_eq!(
            para_text(&section.paragraphs[0]),
            Some("leading and trailing")
        );
    }

    #[test]
    fn paragraph_text_preserves_edge_whitespace_with_xml_space_preserve() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run>
                    <hp:t xml:space="preserve">  leading and trailing  </hp:t>
                </hp:run>
            </hp:p>
        "#,
        );
        let section = parse_test_section(&xml);

        assert_eq!(section.paragraphs.len(), 1);
        assert_eq!(
            para_text(&section.paragraphs[0]),
            Some("  leading and trailing  ")
        );
    }

    #[test]
    fn paragraph_line_segments_are_preserved() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run><hp:t>abcdef</hp:t></hp:run>
                <hp:linesegarray>
                    <hp:lineseg textpos="0" vertpos="10" vertsize="100" textheight="80" baseline="70" spacing="20" horzpos="30" horzsize="400" flags="393216"/>
                    <hp:lineseg textpos="3" vertpos="110" vertsize="120" textheight="90" baseline="75" spacing="25" horzpos="40" horzsize="410" flags="1441792"/>
                </hp:linesegarray>
            </hp:p>
        "#,
        );

        let section = parse_test_section(&xml);

        let segments =
            para_line_segments(&section.paragraphs[0]).expect("line segments should be preserved");

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text_start_position, 0);
        assert_eq!(segments[0].vertical_position, 10);
        assert_eq!(segments[0].line_height, 100);
        assert_eq!(segments[0].text_height, 80);
        assert_eq!(segments[0].baseline_distance, 70);
        assert_eq!(segments[0].line_spacing, 20);
        assert_eq!(segments[0].column_start_position, 30);
        assert_eq!(segments[0].segment_width, 400);
        assert!(segments[0].tag.is_first_segment_of_line);
        assert!(segments[0].tag.is_last_segment_of_line);
        assert_eq!(segments[1].text_start_position, 3);
        assert!(segments[1].tag.has_indentation);
    }

    #[test]
    fn table_cell_paragraph_line_segments_are_preserved() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:tr>
                    <hp:tc>
                        <hp:subList>
                            <hp:p>
                                <hp:run><hp:t>cell text</hp:t></hp:run>
                                <hp:linesegarray>
                                    <hp:lineseg textpos="0" vertpos="5" vertsize="50" textheight="40" baseline="35" spacing="10" horzpos="15" horzsize="200" flags="393216"/>
                                </hp:linesegarray>
                            </hp:p>
                        </hp:subList>
                        <hp:cellAddr colAddr="0" rowAddr="0"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                    </hp:tc>
                </hp:tr>
            </hp:tbl>
        "#,
        );

        let section = parse_test_section(&xml);
        let table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            other => panic!("Expected Table record, got: {other:?}"),
        };

        let segments = para_line_segments(&table.cells[0].paragraphs[0])
            .expect("table cell paragraph line segments should be preserved");

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text_start_position, 0);
        assert_eq!(segments[0].vertical_position, 5);
        assert_eq!(segments[0].segment_width, 200);
    }

    #[test]
    fn invalid_line_segment_numeric_attribute_records_diagnostic() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run><hp:t>abcdef</hp:t></hp:run>
                <hp:linesegarray>
                    <hp:lineseg textpos="bad" vertpos="10" vertsize="100" textheight="80" baseline="70" spacing="20" horzpos="30" horzsize="400" flags="393216"/>
                </hp:linesegarray>
            </hp:p>
        "#,
        );
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let section = parse_section_xml(&xml, 7, &mut warnings, &mut diagnostics).unwrap();
        let segments =
            para_line_segments(&section.paragraphs[0]).expect("line segment should be preserved");

        assert_eq!(segments[0].text_start_position, 0);
        assert!(diagnostics.items.iter().any(|item| {
            item.category == DiagnosticCategory::InvalidValue
                && item.context.section_index == Some(7)
                && item.context.element.as_deref() == Some("hp:lineseg")
                && item.context.attribute.as_deref() == Some("textpos")
                && item.context.value.as_deref() == Some("bad")
        }));
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
        let section = parse_test_section(&xml);
        assert_eq!(section.paragraphs.len(), 3);
        assert_eq!(para_text(&section.paragraphs[0]), Some("First"));
        assert_eq!(para_text(&section.paragraphs[1]), Some("Second"));
        assert_eq!(para_text(&section.paragraphs[2]), Some("Third"));
    }

    #[test]
    fn nested_paragraphs_inside_container_preserve_line_breaks() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:container>
                    <hp:p><hp:run><hp:t>First</hp:t></hp:run></hp:p>
                    <hp:p><hp:run><hp:t>Second</hp:t></hp:run></hp:p>
                </hp:container>
            </hp:p>
        "#,
        );
        let section = parse_test_section(&xml);

        assert_eq!(section.paragraphs.len(), 1);
        assert_eq!(para_text(&section.paragraphs[0]), Some("First\nSecond"));
    }

    #[test]
    fn test_parse_empty_section() {
        let xml = wrap_section("");
        let section = parse_test_section_at(&xml, 5);
        assert_eq!(section.index, 5);
        assert!(section.paragraphs.is_empty());
    }

    #[test]
    fn empty_top_level_paragraphs_are_preserved_with_style_attributes() {
        let xml = wrap_section(
            r#"
            <hp:p prIDRef="4" styleIDRef="2"></hp:p>
            <hp:p prIDRef="5" styleIDRef="3"/>
        "#,
        );

        let section = parse_test_section(&xml);

        assert_eq!(section.paragraphs.len(), 2);
        assert_eq!(para_text(&section.paragraphs[0]), Some(""));
        assert_eq!(section.paragraphs[0].para_header.text_char_count, 0);
        assert_eq!(section.paragraphs[0].para_header.para_shape_id, 4);
        assert_eq!(section.paragraphs[0].para_header.para_style_id, 2);
        assert_eq!(para_text(&section.paragraphs[1]), Some(""));
        assert_eq!(section.paragraphs[1].para_header.text_char_count, 0);
        assert_eq!(section.paragraphs[1].para_header.para_shape_id, 5);
        assert_eq!(section.paragraphs[1].para_header.para_style_id, 3);
    }

    #[test]
    fn parse_section_rejects_non_section_root_element() {
        let xml = r#"
            <root xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
                <hp:p><hp:run><hp:t>Wrong root</hp:t></hp:run></hp:p>
            </root>
        "#;

        let err = parse_test_section_result(xml, 0)
            .expect_err("section XML root element should be hs:sec");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("HWPX section XML root element must be hs:sec")
        ));
    }

    #[test]
    fn parse_section_rejects_multiple_section_roots() {
        let xml = format!(
            "{}{}",
            wrap_section(r#"<hp:p><hp:run><hp:t>First</hp:t></hp:run></hp:p>"#),
            wrap_section(r#"<hp:p><hp:run><hp:t>Second</hp:t></hp:run></hp:p>"#),
        );

        let err = parse_test_section_result(&xml, 0)
            .expect_err("section XML should contain exactly one hs:sec root");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("HWPX section XML contains multiple root elements")
        ));
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
        let section = parse_test_section(&xml);
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
        let section = parse_test_section(&xml);
        assert_eq!(section.paragraphs.len(), 1);
        assert_eq!(section.paragraphs[0].para_header.para_shape_id, 3);
        assert_eq!(section.paragraphs[0].para_header.para_style_id, 2);
    }

    #[test]
    fn para_pr_id_ref_sets_paragraph_shape_id() {
        let xml = wrap_section(
            r#"
            <hp:p paraPrIDRef="7" styleIDRef="3">
                <hp:run><hp:t>Styled paragraph</hp:t></hp:run>
            </hp:p>
        "#,
        );

        let section = parse_test_section(&xml);

        assert_eq!(section.paragraphs.len(), 1);
        assert_eq!(section.paragraphs[0].para_header.para_shape_id, 7);
        assert_eq!(section.paragraphs[0].para_header.para_style_id, 3);
    }

    #[test]
    fn invalid_para_pr_id_ref_records_diagnostic() {
        let xml = wrap_section(
            r#"
            <hp:p paraPrIDRef="bad" styleIDRef="2">
                <hp:run><hp:t>Styled paragraph</hp:t></hp:run>
            </hp:p>
        "#,
        );
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let section = parse_section_xml(&xml, 6, &mut warnings, &mut diagnostics).unwrap();

        assert_eq!(section.paragraphs[0].para_header.para_shape_id, 0);
        assert_eq!(section.paragraphs[0].para_header.para_style_id, 2);
        assert!(diagnostics.items.iter().any(|item| {
            item.category == DiagnosticCategory::InvalidValue
                && item.context.section_index == Some(6)
                && item.context.element.as_deref() == Some("hp:p")
                && item.context.attribute.as_deref() == Some("paraPrIDRef")
                && item.context.value.as_deref() == Some("bad")
        }));
    }

    #[test]
    fn style_id_ref_over_u8_range_records_diagnostic_instead_of_wrapping() {
        let xml = wrap_section(
            r#"
            <hp:p prIDRef="3" styleIDRef="300">
                <hp:run><hp:t>Styled paragraph</hp:t></hp:run>
            </hp:p>
        "#,
        );
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let section = parse_section_xml(&xml, 0, &mut warnings, &mut diagnostics).unwrap();

        assert_eq!(section.paragraphs[0].para_header.para_style_id, 0);
        assert!(diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::RecoveredError
                && item.category == DiagnosticCategory::InvalidValue
                && item.context.attribute.as_deref() == Some("styleIDRef")
                && item.context.value.as_deref() == Some("300")
        }));
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
        let section = parse_test_section(&xml);
        assert_eq!(section.paragraphs[0].para_header.para_shape_id, 0);
        assert_eq!(section.paragraphs[0].para_header.para_style_id, 0);
    }

    // ===== Tab tests =====

    #[test]
    fn invalid_tab_width_records_diagnostic() {
        let xml = r#"
            <hp:sec>
              <hp:p>
                <hp:run>
                  <hp:tab width="wide" leader="bad"/>
                </hp:run>
              </hp:p>
            </hp:sec>
        "#;

        let mut warnings = ParseWarnings::new();
        let mut diagnostics = crate::diagnostics::DiagnosticReport::default();
        let section = parse_section_xml(xml, 0, &mut warnings, &mut diagnostics).unwrap();

        assert_eq!(section.paragraphs.len(), 1);
        let summary = diagnostics.summary();
        assert_eq!(summary.by_severity.get("RecoveredError"), Some(&2));
        assert_eq!(summary.by_category.get("InvalidValue"), Some(&2));
        assert!(diagnostics
            .items
            .iter()
            .any(|item| item.context.attribute.as_deref() == Some("width")));
        assert!(diagnostics
            .items
            .iter()
            .any(|item| item.context.attribute.as_deref() == Some("leader")));
    }

    #[test]
    fn unsupported_section_element_records_diagnostic() {
        let xml = r#"
            <hp:sec>
              <hp:p>
                <hp:run>
                  <hp:chart id="chart1"/>
                </hp:run>
              </hp:p>
            </hp:sec>
        "#;

        let mut warnings = ParseWarnings::new();
        let mut diagnostics = crate::diagnostics::DiagnosticReport::default();
        let _section = parse_section_xml(xml, 3, &mut warnings, &mut diagnostics).unwrap();

        assert!(diagnostics.items.iter().any(|item| {
            item.severity == crate::diagnostics::DiagnosticSeverity::Unsupported
                && item.category == crate::diagnostics::DiagnosticCategory::UnsupportedElement
                && item.context.section_index == Some(3)
                && item.context.element.as_deref() == Some("chart")
        }));
    }

    #[test]
    fn unsupported_section_elements_are_aggregated_by_local_name() {
        let xml = r#"
            <hp:sec xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph"
                    xmlns:a="urn:a"
                    xmlns:b="urn:b">
              <hp:p>
                <hp:run>
                  <a:chart/>
                  <b:chart/>
                  <chart/>
                </hp:run>
              </hp:p>
            </hp:sec>
        "#;

        let mut warnings = ParseWarnings::new();
        let mut diagnostics = crate::diagnostics::DiagnosticReport::default();
        let _section = parse_section_xml(xml, 2, &mut warnings, &mut diagnostics).unwrap();

        let matching: Vec<_> = diagnostics
            .items
            .iter()
            .filter(|item| {
                item.severity == crate::diagnostics::DiagnosticSeverity::Unsupported
                    && item.category == crate::diagnostics::DiagnosticCategory::UnsupportedElement
                    && item.context.element.as_deref() == Some("chart")
            })
            .collect();

        assert_eq!(matching.len(), 1);
        assert!(matching[0].message.contains("occurred 3 time(s)"));
    }

    #[test]
    fn unsupported_field_type_records_diagnostic() {
        let xml = r#"
            <hp:sec>
              <hp:p>
                <hp:run>
                  <hp:fieldBegin type="DATE" id="0">
                    <hp:parameters></hp:parameters>
                  </hp:fieldBegin>
                  <hp:t>2026-05-26</hp:t>
                  <hp:fieldEnd beginIDRef="0"/>
                </hp:run>
              </hp:p>
            </hp:sec>
        "#;

        let mut warnings = ParseWarnings::new();
        let mut diagnostics = crate::diagnostics::DiagnosticReport::default();
        let _section = parse_section_xml(xml, 1, &mut warnings, &mut diagnostics).unwrap();

        assert!(diagnostics.items.iter().any(|item| {
            item.severity == crate::diagnostics::DiagnosticSeverity::Unsupported
                && item.category == crate::diagnostics::DiagnosticCategory::UnsupportedAttribute
                && item.context.element.as_deref() == Some("hp:fieldBegin")
                && item.context.attribute.as_deref() == Some("type")
                && item.context.value.as_deref() == Some("DATE")
        }));
    }

    #[test]
    fn test_parse_tab_element() {
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
        let section = parse_test_section(&xml);
        assert_eq!(section.paragraphs.len(), 1);
        assert_eq!(para_text(&section.paragraphs[0]), Some("Before\tAfter"));
    }

    #[test]
    fn test_parse_dot_leader_tab() {
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
        let section = parse_test_section(&xml);
        assert_eq!(para_text(&section.paragraphs[0]), Some("Name......100"));
    }

    #[test]
    fn table_tab_numeric_attribute_entity_references_are_decoded() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:tr>
                    <hp:tc>
                        <hp:subList>
                            <hp:p>
                                <hp:run>
                                    <hp:tab leader="3" width="9&#54;00"/>
                                </hp:run>
                            </hp:p>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
            </hp:tbl>
        "#,
        );
        let section = parse_test_section(&xml);
        let table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            other => panic!("Expected Table record, got: {other:?}"),
        };

        assert_eq!(para_text(&table.cells[0].paragraphs[0]), Some("........"));
    }

    #[test]
    fn malformed_section_numeric_attribute_entity_is_rejected() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run>
                    <hp:tab leader="3" width="9&unknown_entity;600"/>
                </hp:run>
            </hp:p>
        "#,
        );

        let err = parse_test_section_result(&xml, 12)
            .expect_err("malformed numeric attribute entities should be rejected");

        assert!(matches!(
            err,
            HwpError::XmlParseError(message)
                if message.contains("Error decoding XML attribute width")
                    && message.contains("Contents/section12.xml")
                    && message.contains("hp:tab")
                    && message.contains("unknown_entity")
        ));
    }

    // ===== Table tests =====

    #[test]
    fn invalid_cell_addr_records_diagnostics() {
        let xml = r#"
            <hp:sec>
              <hp:tbl>
                <hp:tr>
                  <hp:tc>
                    <hp:cellAddr colAddr="left" rowAddr="top"/>
                    <hp:cellSpan colSpan="1" rowSpan="1"/>
                    <hp:subList>
                      <hp:p><hp:run><hp:t>Cell</hp:t></hp:run></hp:p>
                    </hp:subList>
                  </hp:tc>
                </hp:tr>
              </hp:tbl>
            </hp:sec>
        "#;

        let mut warnings = ParseWarnings::new();
        let mut diagnostics = crate::diagnostics::DiagnosticReport::default();
        let _section = parse_section_xml(xml, 0, &mut warnings, &mut diagnostics).unwrap();

        let matching: Vec<_> = diagnostics
            .items
            .iter()
            .filter(|item| {
                item.severity == crate::diagnostics::DiagnosticSeverity::RecoveredError
                    && item.category == crate::diagnostics::DiagnosticCategory::InvalidValue
                    && item.context.element.as_deref() == Some("hp:cellAddr")
            })
            .collect();

        assert_eq!(matching.len(), 2);
        assert!(matching
            .iter()
            .any(|item| item.context.attribute.as_deref() == Some("colAddr")));
        assert!(matching
            .iter()
            .any(|item| item.context.attribute.as_deref() == Some("rowAddr")));
    }

    #[test]
    fn zero_cell_span_values_record_diagnostics_and_default_to_one() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:tr>
                    <hp:tc>
                        <hp:cellAddr colAddr="0" rowAddr="0"/>
                        <hp:cellSpan colSpan="0" rowSpan="0"/>
                        <hp:subList>
                            <hp:p><hp:run><hp:t>Cell</hp:t></hp:run></hp:p>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
            </hp:tbl>
        "#,
        );
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let section = parse_section_xml(&xml, 0, &mut warnings, &mut diagnostics).unwrap();
        let table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            other => panic!("Expected Table record, got: {other:?}"),
        };

        assert_eq!(table.cells[0].cell_attributes.col_span, 1);
        assert_eq!(table.cells[0].cell_attributes.row_span, 1);

        let matching: Vec<_> = diagnostics
            .items
            .iter()
            .filter(|item| {
                item.severity == DiagnosticSeverity::RecoveredError
                    && item.category == DiagnosticCategory::InvalidValue
                    && item.context.element.as_deref() == Some("hp:cellSpan")
            })
            .collect();

        assert_eq!(matching.len(), 2);
        assert!(matching
            .iter()
            .any(|item| item.context.attribute.as_deref() == Some("colSpan")));
        assert!(matching
            .iter()
            .any(|item| item.context.attribute.as_deref() == Some("rowSpan")));
    }

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
        let section = parse_test_section(&xml);
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
    fn table_cell_empty_paragraphs_are_preserved_with_style_attributes() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:tr>
                    <hp:tc>
                        <hp:cellAddr colAddr="0" rowAddr="0"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p prIDRef="4" styleIDRef="2"></hp:p>
                            <hp:p prIDRef="5" styleIDRef="3"/>
                            <hp:p prIDRef="6" styleIDRef="4">
                                <hp:run><hp:t>Cell</hp:t></hp:run>
                            </hp:p>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
            </hp:tbl>
        "#,
        );

        let section = parse_test_section(&xml);
        let table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            other => panic!("Expected Table record, got: {other:?}"),
        };

        let cell = &table.cells[0];
        assert_eq!(cell.list_header.paragraph_count, 3);
        assert_eq!(cell.paragraphs.len(), 3);
        assert_eq!(para_text(&cell.paragraphs[0]), Some(""));
        assert_eq!(cell.paragraphs[0].para_header.para_shape_id, 4);
        assert_eq!(cell.paragraphs[0].para_header.para_style_id, 2);
        assert_eq!(para_text(&cell.paragraphs[1]), Some(""));
        assert_eq!(cell.paragraphs[1].para_header.para_shape_id, 5);
        assert_eq!(cell.paragraphs[1].para_header.para_style_id, 3);
        assert_eq!(para_text(&cell.paragraphs[2]), Some("Cell"));
        assert_eq!(cell.paragraphs[2].para_header.para_shape_id, 6);
        assert_eq!(cell.paragraphs[2].para_header.para_style_id, 4);
    }

    #[test]
    fn table_cell_paragraphs_inside_outer_paragraph_preserve_text() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run>
                    <hp:tbl>
                        <hp:tr>
                            <hp:tc>
                                <hp:cellAddr colAddr="0" rowAddr="0"/>
                                <hp:cellSpan colSpan="1" rowSpan="1"/>
                                <hp:subList>
                                    <hp:p prIDRef="7" styleIDRef="5">
                                        <hp:run><hp:t>Nested cell text</hp:t></hp:run>
                                    </hp:p>
                                </hp:subList>
                            </hp:tc>
                        </hp:tr>
                    </hp:tbl>
                </hp:run>
            </hp:p>
        "#,
        );

        let section = parse_test_section(&xml);
        let table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            other => panic!("Expected Table record, got: {other:?}"),
        };

        let cell = &table.cells[0];
        assert_eq!(cell.list_header.paragraph_count, 1);
        assert_eq!(cell.paragraphs.len(), 1);
        assert_eq!(para_text(&cell.paragraphs[0]), Some("Nested cell text"));
        assert_eq!(cell.paragraphs[0].para_header.para_shape_id, 7);
        assert_eq!(cell.paragraphs[0].para_header.para_style_id, 5);
    }

    #[test]
    fn table_cell_paragraph_runs_preserve_char_shape_ids() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:tr>
                    <hp:tc>
                        <hp:cellAddr colAddr="0" rowAddr="0"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p>
                                <hp:run charPrIDRef="1"><hp:t>Bold</hp:t></hp:run>
                                <hp:run charPrIDRef="2"><hp:t>Italic</hp:t></hp:run>
                            </hp:p>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
            </hp:tbl>
        "#,
        );

        let section = parse_test_section(&xml);
        let table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            other => panic!("Expected Table record, got: {other:?}"),
        };

        let para = &table.cells[0].paragraphs[0];
        assert_eq!(para_text(para), Some("BoldItalic"));
        let runs = para_runs(para).unwrap();
        assert_eq!(runs.len(), 2);
        assert!(matches!(
            &runs[0],
            ParaTextRun::Text {
                text,
                char_shape_id: Some(1),
            } if text == "Bold"
        ));
        assert!(matches!(
            &runs[1],
            ParaTextRun::Text {
                text,
                char_shape_id: Some(2),
            } if text == "Italic"
        ));
    }

    #[test]
    fn invalid_char_shape_id_ref_records_diagnostic_and_drops_run_style() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run charPrIDRef="bold"><hp:t>Styled text</hp:t></hp:run>
            </hp:p>
        "#,
        );
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let section = parse_section_xml(&xml, 0, &mut warnings, &mut diagnostics).unwrap();

        let runs = para_runs(&section.paragraphs[0]).unwrap();
        assert_eq!(runs.len(), 1);
        assert!(matches!(
            &runs[0],
            ParaTextRun::Text {
                text,
                char_shape_id: None,
            } if text == "Styled text"
        ));
        assert_eq!(warnings.len(), 1);
        assert!(diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::RecoveredError
                && item.category == DiagnosticCategory::InvalidValue
                && item.context.element.as_deref() == Some("hp:run")
                && item.context.attribute.as_deref() == Some("charPrIDRef")
                && item.context.value.as_deref() == Some("bold")
        }));
    }

    #[test]
    fn table_cell_hyperlink_runs_are_preserved() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:tr>
                    <hp:tc>
                        <hp:cellAddr colAddr="0" rowAddr="0"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p>
                                <hp:run charPrIDRef="3">
                                    <hp:fieldBegin type="HYPERLINK" id="0">
                                        <hp:parameters>
                                            <hp:stringParam name="Path">https://example.com/cell</hp:stringParam>
                                        </hp:parameters>
                                    </hp:fieldBegin>
                                    <hp:t>Cell Link</hp:t>
                                    <hp:fieldEnd beginIDRef="0"/>
                                </hp:run>
                            </hp:p>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
            </hp:tbl>
        "#,
        );

        let section = parse_test_section(&xml);
        let table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            other => panic!("Expected Table record, got: {other:?}"),
        };

        let para = &table.cells[0].paragraphs[0];
        assert_eq!(para_text(para), Some("Cell Link"));
        let runs = para_runs(para).unwrap();
        assert_eq!(runs.len(), 1);
        assert!(matches!(
            &runs[0],
            ParaTextRun::Hyperlink {
                text,
                url,
                char_shape_id: Some(3),
            } if text == "Cell Link" && url == "https://example.com/cell"
        ));
    }

    #[test]
    fn table_inside_run_does_not_overwrite_surrounding_char_shape_id() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run charPrIDRef="9">
                    <hp:t>Before</hp:t>
                    <hp:tbl>
                        <hp:tr>
                            <hp:tc>
                                <hp:cellAddr colAddr="0" rowAddr="0"/>
                                <hp:cellSpan colSpan="1" rowSpan="1"/>
                                <hp:subList>
                                    <hp:p>
                                        <hp:run charPrIDRef="1"><hp:t>Cell</hp:t></hp:run>
                                    </hp:p>
                                </hp:subList>
                            </hp:tc>
                        </hp:tr>
                    </hp:tbl>
                    <hp:t>After</hp:t>
                </hp:run>
            </hp:p>
        "#,
        );

        let section = parse_test_section(&xml);
        let surrounding_para = section
            .paragraphs
            .iter()
            .find(|paragraph| para_text(paragraph) == Some("BeforeAfter"))
            .expect("surrounding text paragraph should be preserved");

        let runs = para_runs(surrounding_para).unwrap();
        assert_eq!(runs.len(), 1);
        assert!(matches!(
            &runs[0],
            ParaTextRun::Text {
                text,
                char_shape_id: Some(9),
            } if text == "BeforeAfter"
        ));
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
        let section = parse_test_section(&xml);
        let table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            _ => panic!("Expected Table"),
        };
        assert_eq!(table.attributes.col_count, 2);
        assert_eq!(table.cells[0].cell_attributes.col_span, 2);
    }

    #[test]
    fn table_cell_implicit_column_address_overflow_records_diagnostic() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:tr>
                    <hp:tc>
                        <hp:cellAddr colAddr="65535" rowAddr="0"/>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p><hp:run><hp:t>A</hp:t></hp:run></hp:p>
                        </hp:subList>
                    </hp:tc>
                    <hp:tc>
                        <hp:cellSpan colSpan="1" rowSpan="1"/>
                        <hp:subList>
                            <hp:p><hp:run><hp:t>B</hp:t></hp:run></hp:p>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
            </hp:tbl>
        "#,
        );
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let section = parse_section_xml(&xml, 0, &mut warnings, &mut diagnostics).unwrap();
        let table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            other => panic!("Expected Table record, got: {other:?}"),
        };

        assert_eq!(table.cells[0].cell_attributes.col_address, 65535);
        assert_eq!(table.cells[1].cell_attributes.col_address, 0);
        assert_eq!(table.attributes.col_count, u16::MAX);
        assert!(diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::RecoveredError
                && item.category == DiagnosticCategory::InvalidValue
                && item.context.element.as_deref() == Some("hp:cellSpan")
                && item.context.attribute.as_deref() == Some("colSpan")
                && item.context.value.as_deref() == Some("65535+1")
        }));
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
        let section = parse_test_section(&xml);
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
        let section = parse_test_section(&xml);
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
    fn image_refs_are_canonicalized_to_bindata_stems() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:pic>
                    <hc:img binaryItemIDRef="BinData/image1.jpg" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/>
                </hp:pic>
            </hp:p>
        "#,
        );
        let section = parse_test_section(&xml);

        let image_ref = section.paragraphs.iter().find_map(|paragraph| {
            paragraph.records.iter().find_map(|record| {
                if let ParagraphRecord::HwpxImage {
                    binary_item_ref, ..
                } = record
                {
                    Some(binary_item_ref.as_str())
                } else {
                    None
                }
            })
        });

        assert_eq!(image_ref, Some("image1"));
    }

    #[test]
    fn image_refs_decode_xml_attribute_entities() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:pic>
                    <hc:img binaryItemIDRef="imag&#101;1" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/>
                </hp:pic>
            </hp:p>
        "#,
        );
        let section = parse_test_section(&xml);

        let image_ref = section.paragraphs.iter().find_map(|paragraph| {
            paragraph.records.iter().find_map(|record| {
                if let ParagraphRecord::HwpxImage {
                    binary_item_ref, ..
                } = record
                {
                    Some(binary_item_ref.as_str())
                } else {
                    None
                }
            })
        });

        assert_eq!(image_ref, Some("image1"));
    }

    #[test]
    fn image_attributes_are_preserved() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:pic>
                    <hc:img binaryItemIDRef="image1" bright="-12" contrast="34" effect="GRAY_SCALE" alpha="128"/>
                </hp:pic>
            </hp:p>
        "#,
        );
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();
        let section = parse_section_xml(&xml, 0, &mut warnings, &mut diagnostics).unwrap();

        let image = section.paragraphs.iter().find_map(|paragraph| {
            paragraph.records.iter().find_map(|record| match record {
                ParagraphRecord::HwpxImage {
                    binary_item_ref,
                    brightness,
                    contrast,
                    effect,
                    alpha,
                } => Some((binary_item_ref, brightness, contrast, effect, alpha)),
                _ => None,
            })
        });

        let (binary_item_ref, brightness, contrast, effect, alpha) =
            image.expect("image record should be parsed");
        assert_eq!(binary_item_ref, "image1");
        assert_eq!(*brightness, Some(-12));
        assert_eq!(*contrast, Some(34));
        assert_eq!(effect.as_deref(), Some("GRAY_SCALE"));
        assert_eq!(*alpha, Some(128));
        assert!(diagnostics.items.is_empty());
    }

    #[test]
    fn invalid_image_numeric_attributes_record_diagnostics() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:pic>
                    <hc:img binaryItemIDRef="image1" bright="bad" contrast="bad" alpha="999"/>
                </hp:pic>
            </hp:p>
        "#,
        );
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();
        let section = parse_section_xml(&xml, 2, &mut warnings, &mut diagnostics).unwrap();

        assert!(section.paragraphs.iter().any(|paragraph| {
            paragraph.records.iter().any(|record| {
                matches!(record, ParagraphRecord::HwpxImage { binary_item_ref, .. } if binary_item_ref == "image1")
            })
        }));

        for attribute in ["bright", "contrast", "alpha"] {
            assert!(
                diagnostics.items.iter().any(|item| {
                    item.category == DiagnosticCategory::InvalidValue
                        && item.context.section_index == Some(2)
                        && item.context.element.as_deref() == Some("hc:img")
                        && item.context.attribute.as_deref() == Some(attribute)
                }),
                "expected diagnostic for {attribute}"
            );
        }
    }

    #[test]
    fn unsafe_image_refs_are_skipped_with_diagnostic() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:pic>
                    <hc:img binaryItemIDRef="../secret.png" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/>
                </hp:pic>
            </hp:p>
        "#,
        );
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let section = parse_section_xml(&xml, 4, &mut warnings, &mut diagnostics).unwrap();

        let has_image = section.paragraphs.iter().any(|paragraph| {
            paragraph
                .records
                .iter()
                .any(|record| matches!(record, ParagraphRecord::HwpxImage { .. }))
        });
        assert!(!has_image, "unsafe image refs should not be preserved");
        assert!(diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::DataLoss
                && item.category == DiagnosticCategory::SkippedBinary
                && item.context.section_index == Some(4)
                && item.context.element.as_deref() == Some("hc:img")
                && item.context.attribute.as_deref() == Some("binaryItemIDRef")
                && item.context.value.as_deref() == Some("../secret.png")
        }));
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn image_refs_with_control_characters_are_skipped_with_diagnostic() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:pic>
                    <hc:img binaryItemIDRef="image&#10;1" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/>
                </hp:pic>
            </hp:p>
        "#,
        );
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let section = parse_section_xml(&xml, 5, &mut warnings, &mut diagnostics).unwrap();

        let has_image = section.paragraphs.iter().any(|paragraph| {
            paragraph
                .records
                .iter()
                .any(|record| matches!(record, ParagraphRecord::HwpxImage { .. }))
        });
        assert!(
            !has_image,
            "control characters in image refs should not be preserved"
        );
        assert!(diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::DataLoss
                && item.category == DiagnosticCategory::SkippedBinary
                && item.context.section_index == Some(5)
                && item.context.element.as_deref() == Some("hc:img")
                && item.context.attribute.as_deref() == Some("binaryItemIDRef")
                && item
                    .context
                    .value
                    .as_deref()
                    .is_some_and(|value| value.contains('\n'))
        }));
        assert_eq!(warnings.len(), 1);
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
        let section = parse_test_section(&xml);
        let table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            _ => panic!("Expected Table"),
        };
        // Cell should have an image paragraph
        let cell_has_image = table.cells[0].paragraphs.iter().any(|p| {
            p.records.iter().any(|r| {
                matches!(r, ParagraphRecord::HwpxImage { binary_item_ref, .. } if binary_item_ref == "cellimg")
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
        let section = parse_test_section(&xml);
        assert_eq!(section.paragraphs.len(), 1);

        let runs = para_runs(&section.paragraphs[0]).unwrap();
        let has_hyperlink = runs.iter().any(|r| {
            matches!(r, ParaTextRun::Hyperlink { url, text, .. }
                if url == "https://example.com" && text == "Click here")
        });
        assert!(has_hyperlink, "Should contain a hyperlink run");
    }

    #[test]
    fn hyperlink_control_attributes_decode_xml_entities() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run charPrIDRef="0">
                    <hp:fieldBegin type="HYP&#69;RLINK" id="0">
                        <hp:parameters>
                            <hp:stringParam name="P&#97;th">https://example.com/entity</hp:stringParam>
                        </hp:parameters>
                    </hp:fieldBegin>
                    <hp:t>Decoded link</hp:t>
                    <hp:fieldEnd beginIDRef="0"/>
                </hp:run>
            </hp:p>
        "#,
        );
        let section = parse_test_section(&xml);
        assert_eq!(section.paragraphs.len(), 1);

        let runs = para_runs(&section.paragraphs[0]).unwrap();
        let has_hyperlink = runs.iter().any(|r| {
            matches!(r, ParaTextRun::Hyperlink { url, text, .. }
                if url == "https://example.com/entity" && text == "Decoded link")
        });
        assert!(has_hyperlink, "Should contain a decoded hyperlink run");
    }

    #[test]
    fn hyperlink_path_mixed_text_and_cdata_is_concatenated() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run charPrIDRef="0">
                    <hp:fieldBegin type="HYPERLINK" id="0">
                        <hp:parameters>
                            <hp:stringParam name="Path">https://example.com/<![CDATA[cdata]]>/end</hp:stringParam>
                        </hp:parameters>
                    </hp:fieldBegin>
                    <hp:t>Mixed URL</hp:t>
                    <hp:fieldEnd beginIDRef="0"/>
                </hp:run>
            </hp:p>
        "#,
        );
        let section = parse_test_section(&xml);
        let runs = para_runs(&section.paragraphs[0]).unwrap();

        assert!(runs.iter().any(|r| {
            matches!(r, ParaTextRun::Hyperlink { url, text, .. }
                if url == "https://example.com/cdata/end" && text == "Mixed URL")
        }));
    }

    #[test]
    fn hyperlink_without_path_preserves_visible_text_as_plain_run() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run charPrIDRef="1">
                    <hp:fieldBegin type="HYPERLINK" id="0">
                        <hp:parameters>
                            <hp:stringParam name="Tooltip">No URL here</hp:stringParam>
                        </hp:parameters>
                    </hp:fieldBegin>
                    <hp:t>Visible text</hp:t>
                    <hp:fieldEnd beginIDRef="0"/>
                </hp:run>
            </hp:p>
        "#,
        );
        let section = parse_test_section(&xml);
        let runs = para_runs(&section.paragraphs[0]).unwrap();

        assert_eq!(runs.len(), 1);
        assert!(matches!(
            &runs[0],
            ParaTextRun::Text {
                text,
                char_shape_id: Some(1),
            } if text == "Visible text"
        ));
        assert!(!runs
            .iter()
            .any(|r| matches!(r, ParaTextRun::Hyperlink { .. })));
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
        let section = parse_test_section(&xml);
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
        let section = parse_test_section(&xml);
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
        let section = parse_test_section(&xml);
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
        let section = parse_test_section_at(&xml, 42);
        assert_eq!(section.index, 42);
    }

    #[test]
    fn parse_sections_preserves_numeric_section_file_index() {
        let xml = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Fifth</hp:t></hp:run></hp:p>
        "#,
        );
        let data = zip_with_files(&[("Contents/section5.xml", xml.as_bytes())]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let body_text = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect("numeric section file should parse");

        assert_eq!(body_text.sections.len(), 1);
        assert_eq!(body_text.sections[0].index, 5);
    }

    #[test]
    fn parse_sections_uses_content_hpf_spine_order() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Zero</hp:t></hp:run></hp:p>
        "#,
        );
        let section1 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>One</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="header" href="Contents/header.xml" media-type="application/xml"/>
                <opf:item id="first" href="Contents/section0.xml" media-type="application/xml"/>
                <opf:item id="second" href="Contents/section1.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="header"/>
                <opf:itemref idref="second"/>
                <opf:itemref idref="first"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
            ("Contents/section1.xml", section1.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let body_text = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect("content.hpf spine order should parse");

        assert_eq!(body_text.sections.len(), 2);
        assert_eq!(body_text.sections[0].index, 1);
        assert_eq!(body_text.sections[1].index, 0);
        assert_eq!(para_text(&body_text.sections[0].paragraphs[0]), Some("One"));
        assert_eq!(
            para_text(&body_text.sections[1].paragraphs[0]),
            Some("Zero")
        );
    }

    #[test]
    fn parse_sections_resolves_content_hpf_relative_section_hrefs() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Relative</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section" href="section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let body_text = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect("content.hpf hrefs relative to Contents/ should parse");

        assert_eq!(body_text.sections.len(), 1);
        assert_eq!(body_text.sections[0].index, 0);
        assert_eq!(
            para_text(&body_text.sections[0].paragraphs[0]),
            Some("Relative"),
        );
    }

    #[test]
    fn parse_sections_ignores_content_hpf_item_elements_outside_manifest_and_spine() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Scoped</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:metadata>
                <opf:item id="metadata-noise"/>
                <opf:itemref linear="yes"/>
              </opf:metadata>
              <opf:manifest>
                <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section0"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let body_text = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect("item-like elements outside manifest/spine should be ignored");

        assert_eq!(body_text.sections.len(), 1);
        assert_eq!(
            para_text(&body_text.sections[0].paragraphs[0]),
            Some("Scoped"),
        );
    }

    #[test]
    fn parse_sections_rejects_content_hpf_with_only_nested_manifest_sections() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Nested manifest</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:metadata>
                <opf:manifest>
                  <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
                </opf:manifest>
                <opf:spine>
                  <opf:itemref idref="section0"/>
                </opf:spine>
              </opf:metadata>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("nested manifest/spine blocks should not define body sections");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("content.hpf does not list HWPX section file")
                    && reason.contains("Contents/section0.xml")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_with_nested_manifest_item_entry() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Nested manifest item</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:group>
                  <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
                </opf:group>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section0"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("nested manifest item entries should not define body sections");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("content.hpf spine itemref references unknown manifest id")
                    && reason.contains("section0")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_with_nested_spine_itemref_entry() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Nested spine itemref</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:group>
                  <opf:itemref idref="section0"/>
                </opf:group>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("nested spine itemref entries should not define body sections");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("content.hpf does not list HWPX section file")
                    && reason.contains("Contents/section0.xml")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_with_multiple_package_roots() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Multiple package roots</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
            </opf:package>
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:spine>
                <opf:itemref idref="section0"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf should contain exactly one package root element");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("content.hpf contains multiple package root elements")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_with_multiple_direct_manifest_elements() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>First manifest</hp:t></hp:run></hp:p>
        "#,
        );
        let section1 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Second manifest</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:manifest>
                <opf:item id="section1" href="Contents/section1.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section0"/>
                <opf:itemref idref="section1"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
            ("Contents/section1.xml", section1.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf should contain at most one direct manifest element");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("content.hpf contains multiple manifest elements")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_with_multiple_direct_spine_elements() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>First spine</hp:t></hp:run></hp:p>
        "#,
        );
        let section1 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Second spine</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
                <opf:item id="section1" href="Contents/section1.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section0"/>
              </opf:spine>
              <opf:spine>
                <opf:itemref idref="section1"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
            ("Contents/section1.xml", section1.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf should contain at most one direct spine element");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("content.hpf contains multiple spine elements")
        ));
    }

    #[test]
    fn parse_sections_rejects_unsafe_content_hpf_section_href() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Unsafe href target</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section" href="Other/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("unsafe section hrefs in content.hpf should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("Unsafe content.hpf section href")
                    && reason.contains("Other/section0.xml")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_section_like_href_without_numeric_index() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Valid section</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="draft" href="sectiondraft.xml" media-type="application/xml"/>
                <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="draft"/>
                <opf:itemref idref="section0"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("section-like content.hpf hrefs must have numeric section indexes");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("Invalid content.hpf section href")
                    && reason.contains("sectiondraft.xml")
        ));
    }

    #[test]
    fn parse_sections_rejects_unsafe_content_hpf_manifest_href_even_when_not_a_section() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Unsafe manifest href</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="metadata" href="../metadata.xml" media-type="application/xml"/>
                <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section0"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("unsafe manifest hrefs in content.hpf should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("Unsafe content.hpf manifest href")
                    && reason.contains("../metadata.xml")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_manifest_item_count_over_limit() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Manifest item budget</hp:t></hp:run></hp:p>
        "#,
        );
        let mut content_hpf = String::from(
            r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest>"#,
        );
        content_hpf.push_str(
            r#"<opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>"#,
        );
        for index in 0..MAX_HWPX_CONTENT_HPF_MANIFEST_ITEMS {
            content_hpf.push_str(&format!(
                r#"<opf:item id="metadata{index}" href="metadata{index}.xml" media-type="application/xml"/>"#
            ));
        }
        content_hpf.push_str(
            r#"</opf:manifest><opf:spine><opf:itemref idref="section0"/></opf:spine></opf:package>"#,
        );
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf manifest item counts over the limit should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX content.hpf manifest item count"
                && path == "Contents/content.hpf"
                && limit == MAX_HWPX_CONTENT_HPF_MANIFEST_ITEMS
                && actual == MAX_HWPX_CONTENT_HPF_MANIFEST_ITEMS + 1
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_spine_itemref_count_over_limit() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Spine itemref budget</hp:t></hp:run></hp:p>
        "#,
        );
        let mut content_hpf = String::from(
            r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf/"><opf:manifest>"#,
        );
        content_hpf.push_str(
            r#"<opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>"#,
        );
        content_hpf.push_str(r#"</opf:manifest><opf:spine>"#);
        for _ in 0..=MAX_HWPX_CONTENT_HPF_SPINE_ITEMREFS {
            content_hpf.push_str(r#"<opf:itemref idref="section0"/>"#);
        }
        content_hpf.push_str(r#"</opf:spine></opf:package>"#);
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf spine itemref counts over the limit should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX content.hpf spine itemref count"
                && path == "Contents/content.hpf"
                && limit == MAX_HWPX_CONTENT_HPF_SPINE_ITEMREFS
                && actual == MAX_HWPX_CONTENT_HPF_SPINE_ITEMREFS + 1
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_that_omits_archive_section() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Included</hp:t></hp:run></hp:p>
        "#,
        );
        let section1 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Omitted</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section0"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
            ("Contents/section1.xml", section1.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf should not omit section files present in the archive");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("content.hpf does not list HWPX section file")
                    && reason.contains("Contents/section1.xml")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_manifest_item_missing_href() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Missing href</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section0" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section0"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf manifest items missing href should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("content.hpf manifest item missing required href")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_manifest_item_missing_id() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Missing id</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section0"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf manifest items missing id should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("content.hpf manifest item missing required id")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_manifest_item_empty_id() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Empty id</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="" href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref=""/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf manifest item ids must not be empty");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("Invalid content.hpf manifest item id")
                    && reason.contains("empty")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_manifest_item_oversized_href() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Oversized href</hp:t></hp:run></hp:p>
        "#,
        );
        let oversized_href = format!("{}.xml", "a".repeat(1025));
        let content_hpf = format!(
            r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="metadata" href="{oversized_href}" media-type="application/xml"/>
                <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section0"/>
              </opf:spine>
            </opf:package>
        "#
        );
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("oversized content.hpf manifest hrefs should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX content.hpf reference bytes"
                && path == "Contents/content.hpf"
                && limit == 1024
                && actual > 1024
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_spine_itemref_decoded_whitespace_idref() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Whitespace idref</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section&#10;0"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf spine idrefs with decoded whitespace should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("Invalid content.hpf spine itemref idref")
                    && reason.contains("whitespace")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_spine_itemref_invalid_linear_value() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Invalid linear</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section0" linear="maybe"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf spine itemref linear values must be yes, no, or omitted");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("Invalid content.hpf spine itemref linear")
                    && reason.contains("maybe")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_duplicate_spine_idref() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Duplicate spine idref</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section0"/>
                <opf:itemref idref="section0"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf spine itemrefs should not duplicate section idrefs");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("Duplicate content.hpf spine itemref idref")
                    && reason.contains("section0")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_duplicate_section_href() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Duplicate section href</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section0a" href="Contents/section0.xml" media-type="application/xml"/>
                <opf:item id="section0b" href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section0a"/>
                <opf:itemref idref="section0b"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics).expect_err(
            "content.hpf should not list the same section href through multiple idrefs",
        );

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("Duplicate content.hpf section href")
                    && reason.contains("Contents/section0.xml")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_section_item_with_non_xml_media_type() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Wrong media type</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section0" href="Contents/section0.xml" media-type="image/png"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section0"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf section item media-type must be XML-compatible");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("Invalid content.hpf section media-type")
                    && reason.contains("image/png")
                    && reason.contains("Contents/section0.xml")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_section_item_missing_media_type() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Missing media type</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section0" href="Contents/section0.xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref idref="section0"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf section item media-type is required");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("content.hpf section item missing required media-type")
                    && reason.contains("Contents/section0.xml")
        ));
    }

    #[test]
    fn parse_sections_rejects_content_hpf_spine_itemref_missing_idref() {
        let section0 = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Missing idref</hp:t></hp:run></hp:p>
        "#,
        );
        let content_hpf = r#"
            <opf:package xmlns:opf="http://www.idpf.org/2007/opf/">
              <opf:manifest>
                <opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>
              </opf:manifest>
              <opf:spine>
                <opf:itemref linear="yes"/>
              </opf:spine>
            </opf:package>
        "#;
        let data = zip_with_files(&[
            ("Contents/content.hpf", content_hpf.as_bytes()),
            ("Contents/section0.xml", section0.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("content.hpf spine itemrefs missing idref should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("content.hpf spine itemref missing required idref")
        ));
    }

    #[test]
    fn parse_sections_rejects_duplicate_numeric_section_indices() {
        let xml = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Duplicate</hp:t></hp:run></hp:p>
        "#,
        );
        let data = zip_with_files(&[
            ("Contents/section1.xml", xml.as_bytes()),
            ("Contents/section001.xml", xml.as_bytes()),
        ]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("duplicate numeric section indexes should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("Duplicate section index")
                    && reason.contains("Contents/section1.xml")
                    && reason.contains("Contents/section001.xml")
        ));
    }

    #[test]
    fn parse_sections_rejects_section_index_over_word_range() {
        let xml = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>Overflow</hp:t></hp:run></hp:p>
        "#,
        );
        let data = zip_with_files(&[("Contents/section65536.xml", xml.as_bytes())]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("section indexes outside WORD range should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("Section index exceeds WORD range")
                    && reason.contains("Contents/section65536.xml")
        ));
    }

    #[test]
    fn parse_sections_rejects_oversized_section_xml_before_parsing() {
        let oversized = "x".repeat((96 * 1024 * 1024) + 1);
        let data = zip_with_stored_files(&[("Contents/section0.xml", oversized.as_bytes())]);
        let mut container = HwpxContainer::open(&data).unwrap();
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_sections(&mut container, &mut warnings, &mut diagnostics)
            .expect_err("oversized section XML should be rejected before XML parsing");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX section XML byte size"
                && path == "Contents/section0.xml"
                && limit == 96 * 1024 * 1024
                && actual == (96 * 1024 * 1024) + 1
        ));
    }

    // ===== Error handling tests =====

    #[test]
    fn test_parse_malformed_xml() {
        let xml = "<broken><unclosed>";
        // Should not panic, returns error or empty section
        let result = parse_test_section_result(xml, 0);
        // Malformed XML might still parse (quick_xml is lenient) or error
        // Either way it should not panic
        let _ = result;
    }

    #[test]
    fn invalid_text_entity_is_rejected_instead_of_dropped() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run>
                    <hp:t>&unknown_entity;</hp:t>
                </hp:run>
            </hp:p>
        "#,
        );

        let err = parse_test_section_result(&xml, 11)
            .expect_err("invalid XML text entities should be rejected");

        assert!(matches!(
            err,
            HwpError::XmlParseError(message)
                if message.contains("Error unescaping text in Contents/section11.xml")
                    && message.contains("unknown_entity")
        ));
    }

    #[test]
    fn duplicate_section_attribute_is_rejected() {
        let xml = wrap_section(
            r#"
            <hp:p prIDRef="1" prIDRef="2">
                <hp:run><hp:t>Duplicate attribute</hp:t></hp:run>
            </hp:p>
        "#,
        );

        let err = parse_test_section_result(&xml, 3)
            .expect_err("duplicate section XML attributes should be rejected");

        assert!(matches!(
            err,
            HwpError::XmlParseError(message)
                if message.contains("attribute")
                    && message.contains("Contents/section3.xml")
                    && message.contains("hp:p")
        ));
    }

    #[test]
    fn rejects_section_xml_with_excessive_nesting_depth() {
        let mut body = String::new();
        for _ in 0..MAX_HWPX_XML_DEPTH {
            body.push_str("<hp:run>");
        }
        for _ in 0..MAX_HWPX_XML_DEPTH {
            body.push_str("</hp:run>");
        }

        let xml = wrap_section(&body);
        let err = parse_test_section_result(&xml, 7)
            .expect_err("section XML with excessive nesting should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX XML nesting depth"
                && path == "Contents/section7.xml"
                && limit == MAX_HWPX_XML_DEPTH as u64
                && actual == MAX_HWPX_XML_DEPTH as u64 + 1
        ));
    }

    #[test]
    fn rejects_section_xml_event_count_over_budget() {
        let err = validate_xml_event_count("Contents/section0.xml", MAX_HWPX_XML_EVENTS + 1)
            .expect_err("section XML event count over budget should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX XML event count"
                && path == "Contents/section0.xml"
                && limit == MAX_HWPX_XML_EVENTS
                && actual == MAX_HWPX_XML_EVENTS + 1
        ));
    }

    #[test]
    fn rejects_section_xml_when_paragraph_count_exceeds_limit() {
        let xml = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>First</hp:t></hp:run></hp:p>
            <hp:p><hp:run><hp:t>Second</hp:t></hp:run></hp:p>
        "#,
        );
        let limits = SectionStructureLimits {
            max_paragraphs: 1,
            ..Default::default()
        };
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_section_xml_with_limits(&xml, 9, &mut warnings, &mut diagnostics, limits)
            .expect_err("section paragraph count over budget should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX section paragraph count"
                && path == "Contents/section9.xml"
                && limit == 1
                && actual == 2
        ));
    }

    #[test]
    fn rejects_section_xml_when_text_bytes_exceed_limit() {
        let xml = wrap_section(
            r#"
            <hp:p><hp:run><hp:t>abcdef</hp:t></hp:run></hp:p>
        "#,
        );
        let limits = SectionStructureLimits {
            max_text_bytes: 5,
            ..Default::default()
        };
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_section_xml_with_limits(&xml, 4, &mut warnings, &mut diagnostics, limits)
            .expect_err("section text over budget should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX section text bytes"
                && path == "Contents/section4.xml"
                && limit == 5
                && actual == 6
        ));
    }

    #[test]
    fn rejects_section_xml_when_text_run_count_per_paragraph_exceeds_limit() {
        let xml = wrap_section(
            r#"
            <hp:p>
                <hp:run charPrIDRef="1"><hp:t>a</hp:t></hp:run>
                <hp:run charPrIDRef="2"><hp:t>b</hp:t></hp:run>
                <hp:run charPrIDRef="3"><hp:t>c</hp:t></hp:run>
            </hp:p>
        "#,
        );
        let limits = SectionStructureLimits {
            max_text_runs_per_paragraph: 2,
            ..Default::default()
        };
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_section_xml_with_limits(&xml, 10, &mut warnings, &mut diagnostics, limits)
            .expect_err("section text run count over budget should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX section text run count per paragraph"
                && path == "Contents/section10.xml"
                && limit == 2
                && actual == 3
        ));
    }

    #[test]
    fn text_run_limit_resets_between_table_cell_paragraphs() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:tr>
                    <hp:tc>
                        <hp:subList>
                            <hp:p><hp:run><hp:t>a</hp:t></hp:run></hp:p>
                            <hp:p><hp:run><hp:t>b</hp:t></hp:run></hp:p>
                        </hp:subList>
                    </hp:tc>
                </hp:tr>
            </hp:tbl>
        "#,
        );
        let limits = SectionStructureLimits {
            max_text_runs_per_paragraph: 1,
            ..Default::default()
        };
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let section =
            parse_section_xml_with_limits(&xml, 14, &mut warnings, &mut diagnostics, limits)
                .expect("run count budget should reset between table cell paragraphs");

        let table = match &section.paragraphs[0].records[0] {
            ParagraphRecord::Table { table } => table,
            other => panic!("Expected Table record, got: {other:?}"),
        };
        assert_eq!(table.cells[0].paragraphs.len(), 2);
        assert_eq!(para_text(&table.cells[0].paragraphs[0]), Some("a"));
        assert_eq!(para_text(&table.cells[0].paragraphs[1]), Some("b"));
    }

    #[test]
    fn paragraph_text_char_count_over_u32_range_is_rejected() {
        let err = paragraph_text_char_count(u32::MAX as usize + 1, "Contents/section13.xml")
            .expect_err("paragraph text character counts over ParaHeader width should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX paragraph text character count"
                && path == "Contents/section13.xml"
                && limit == u32::MAX as u64
                && actual == u32::MAX as u64 + 1
        ));
    }

    #[test]
    fn rejects_section_xml_when_table_row_count_exceeds_limit() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:tr><hp:tc></hp:tc></hp:tr>
                <hp:tr><hp:tc></hp:tc></hp:tr>
            </hp:tbl>
        "#,
        );
        let limits = SectionStructureLimits {
            max_table_rows: 1,
            ..Default::default()
        };
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_section_xml_with_limits(&xml, 6, &mut warnings, &mut diagnostics, limits)
            .expect_err("section table row count over budget should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX section table row count"
                && path == "Contents/section6.xml"
                && limit == 1
                && actual == 2
        ));
    }

    #[test]
    fn default_table_row_limit_fits_document_row_count_width() {
        assert_eq!(MAX_HWPX_SECTION_TABLE_ROWS, u16::MAX as u64);
    }

    #[test]
    fn create_table_from_rows_rejects_row_count_over_document_width() {
        let rows = vec![Vec::<HwpxCell>::new(); u16::MAX as usize + 1];
        let mut diagnostics = DiagnosticReport::default();

        let err = create_table_from_rows(rows, "test-section.xml", 0, &mut diagnostics)
            .expect_err("table row count over TableAttributes width should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX table row count"
                && path == "test-section.xml"
                && limit == u16::MAX as u64
                && actual == u16::MAX as u64 + 1
        ));
    }

    #[test]
    fn rejects_section_xml_when_table_cell_paragraph_count_exceeds_list_header_width() {
        let mut body = String::from("<hp:tbl><hp:tr><hp:tc><hp:subList>");
        for _ in 0..=(i16::MAX as usize) {
            body.push_str("<hp:p><hp:run><hp:t>x</hp:t></hp:run></hp:p>");
        }
        body.push_str("</hp:subList></hp:tc></hp:tr></hp:tbl>");

        let xml = wrap_section(&body);
        let err = parse_test_section_result(&xml, 12)
            .expect_err("table cell paragraph count over ListHeader width should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX table cell paragraph count"
                && path == "Contents/section12.xml"
                && limit == i16::MAX as u64
                && actual == i16::MAX as u64 + 1
        ));
    }

    #[test]
    fn rejects_section_xml_when_table_cell_count_exceeds_limit() {
        let xml = wrap_section(
            r#"
            <hp:tbl>
                <hp:tr>
                    <hp:tc></hp:tc>
                    <hp:tc></hp:tc>
                </hp:tr>
            </hp:tbl>
        "#,
        );
        let limits = SectionStructureLimits {
            max_table_cells: 1,
            ..Default::default()
        };
        let mut warnings = ParseWarnings::new();
        let mut diagnostics = DiagnosticReport::default();

        let err = parse_section_xml_with_limits(&xml, 8, &mut warnings, &mut diagnostics, limits)
            .expect_err("section table cell count over budget should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX section table cell count"
                && path == "Contents/section8.xml"
                && limit == 1
                && actual == 2
        ));
    }

    // ===== Helper function tests =====

    #[test]
    fn test_create_paragraph() {
        let para = create_paragraph("Hello", "test-section.xml").unwrap();
        assert_eq!(para.para_header.text_char_count, 5);
        assert_eq!(para_text(&para), Some("Hello"));
        assert!(para_runs(&para).unwrap().is_empty());
    }

    #[test]
    fn create_empty_paragraph_does_not_emit_empty_text_run() {
        let para = create_paragraph("", "test-section.xml").unwrap();

        assert_eq!(para.para_header.text_char_count, 0);
        assert_eq!(para_text(&para), Some(""));
        assert!(para_runs(&para).unwrap().is_empty());
    }

    #[test]
    fn visible_text_len_from_runs_counts_plain_and_hyperlink_text() {
        let runs = vec![
            TextRunInfo {
                text: "abc".to_string(),
                char_shape_id: None,
            },
            TextRunInfo {
                text: "\x00HYPERLINK:https://example.test\x00linked".to_string(),
                char_shape_id: Some(3),
            },
            TextRunInfo {
                text: String::new(),
                char_shape_id: None,
            },
        ];

        assert_eq!(visible_text_len_from_runs(&runs), "abclinked".len());
    }

    #[test]
    fn create_paragraph_with_runs_drains_run_buffer_without_discarding_capacity() {
        let mut runs = Vec::with_capacity(8);
        runs.push(TextRunInfo {
            text: "Plain ".to_string(),
            char_shape_id: Some(1),
        });
        runs.push(TextRunInfo {
            text: "\x00HYPERLINK:https://example.test\x00linked".to_string(),
            char_shape_id: Some(2),
        });
        let original_capacity = runs.capacity();

        let para = create_paragraph_with_runs(&mut runs, "test-section.xml").unwrap();

        assert!(runs.is_empty());
        assert_eq!(runs.capacity(), original_capacity);
        assert_eq!(para.para_header.text_char_count, 12);
        assert_eq!(para_text(&para), Some("Plain linked"));
        let para_runs = para_runs(&para).unwrap();
        assert!(matches!(
            &para_runs[0],
            ParaTextRun::Text {
                text,
                char_shape_id: Some(1),
            } if text == "Plain "
        ));
        assert!(matches!(
            &para_runs[1],
            ParaTextRun::Hyperlink {
                text,
                url,
                char_shape_id: Some(2),
            } if text == "linked" && url == "https://example.test"
        ));
    }

    #[test]
    fn test_create_image_paragraph_fn() {
        let para = create_image_paragraph(&HwpxImageInfo {
            binary_item_ref: "img001".to_string(),
            ..Default::default()
        });
        match &para.records[0] {
            ParagraphRecord::HwpxImage {
                binary_item_ref, ..
            } => {
                assert_eq!(binary_item_ref, "img001");
            }
            _ => panic!("Expected HwpxImage"),
        }
    }

    #[test]
    fn test_create_table_from_rows_fn() {
        let table_rows = vec![vec![
            HwpxCell {
                content_items: vec![CellContentItem::Paragraph(
                    create_paragraph("A", "test-section.xml").unwrap(),
                )],
                col_addr: Some(0),
                row_addr: Some(0),
                ..Default::default()
            },
            HwpxCell {
                content_items: vec![CellContentItem::Paragraph(
                    create_paragraph("B", "test-section.xml").unwrap(),
                )],
                col_addr: Some(1),
                row_addr: Some(0),
                ..Default::default()
            },
        ]];
        let mut diagnostics = DiagnosticReport::default();
        let table =
            create_table_from_rows(table_rows, "test-section.xml", 0, &mut diagnostics).unwrap();
        assert_eq!(table.attributes.row_count, 1);
        assert_eq!(table.attributes.col_count, 2);
        assert_eq!(table.cells.len(), 2);
    }

    #[test]
    fn table_rows_metrics_counts_cells_and_preserves_column_overflow_value() {
        let table_rows = vec![vec![
            HwpxCell {
                content_items: vec![CellContentItem::Paragraph(
                    create_paragraph("A", "test-section.xml").unwrap(),
                )],
                col_addr: Some(u16::MAX),
                col_span: 1,
                ..Default::default()
            },
            HwpxCell {
                content_items: vec![CellContentItem::Paragraph(
                    create_paragraph("B", "test-section.xml").unwrap(),
                )],
                col_addr: Some(1),
                col_span: 2,
                ..Default::default()
            },
        ]];

        let metrics = table_rows_metrics(&table_rows, "test-section.xml").unwrap();

        assert_eq!(metrics.row_count, 1);
        assert_eq!(metrics.total_cells, 2);
        assert_eq!(metrics.col_count, u16::MAX);
        assert_eq!(metrics.invalid_column_extent.as_deref(), Some("65535+1"));
    }
}
