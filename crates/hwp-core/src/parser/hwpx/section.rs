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
use crate::error::HwpError;
use crate::types::{HWPUNIT, UINT16, WORD};

use super::container::HwpxContainer;

/// Cell data with colspan/rowspan and address information
#[derive(Debug, Clone)]
struct HwpxCell {
    text: String,
    col_span: u16,
    row_span: u16,
    col_addr: Option<u16>,
    row_addr: Option<u16>,
}

impl Default for HwpxCell {
    fn default() -> Self {
        Self {
            text: String::new(),
            col_span: 1,
            row_span: 1,
            col_addr: None,
            row_addr: None,
        }
    }
}

/// Parse all section files and create BodyText
pub fn parse_sections(container: &mut HwpxContainer) -> Result<BodyText, HwpError> {
    let section_files = container.get_section_files();

    if section_files.is_empty() {
        return Err(HwpError::InvalidHwpxStructure {
            reason: "No section files found in Contents/".to_string(),
        });
    }

    let mut sections = Vec::new();

    for (index, section_path) in section_files.iter().enumerate() {
        let content = container.read_file_string(section_path)?;
        let section = parse_section_xml(&content, index as WORD)?;
        sections.push(section);
    }

    Ok(BodyText { sections })
}

/// Parse a single section XML file
fn parse_section_xml(content: &str, index: WORD) -> Result<Section, HwpError> {
    let mut reader = Reader::from_str(content);
    reader.config_mut().trim_text(true);

    let mut paragraphs = Vec::new();
    let mut current_text = String::new();
    let mut in_text = false;
    let mut in_table = false;
    let mut in_cell = false;
    let mut in_caption = false;
    let mut _in_picture = false;

    // Image parsing
    let mut current_image_ref: Option<String> = None;

    // Table parsing with colspan/rowspan support
    let mut table_rows: Vec<Vec<HwpxCell>> = Vec::new();
    let mut current_row: Vec<HwpxCell> = Vec::new();
    let mut current_cell = HwpxCell::default();
    let mut table_caption = String::new();

    // Track nesting depth for paragraphs
    let mut para_depth: u32 = 0;

    loop {
        match reader.read_event() {
            Ok(Event::Empty(ref e)) => {
                // Handle self-closing tags like <hp:cellSpan ... />, <hp:cellAddr ... />, <hp:tab ... />
                let name = e.name();
                let local_name = String::from_utf8_lossy(name.as_ref());

                if local_name.ends_with(":tab") || local_name == "tab" {
                    // Parse tab element and convert to appropriate text representation
                    // Tab attributes: width (HWPUNIT), leader (0=none, 1=solid, 2=dash, 3=dot), type
                    let mut leader: u8 = 0;
                    let mut width: u32 = 0;

                    for attr in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(attr.key.as_ref());
                        let value = String::from_utf8_lossy(&attr.value);
                        match key.as_ref() {
                            "leader" => {
                                leader = value.parse().unwrap_or(0);
                            }
                            "width" => {
                                width = value.parse().unwrap_or(0);
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
                    if in_table && in_caption {
                        table_caption.push_str(&tab_text);
                    } else if in_table && in_cell {
                        current_cell.text.push_str(&tab_text);
                    } else if !in_table {
                        current_text.push_str(&tab_text);
                    }
                } else if local_name.ends_with(":cellSpan") || local_name == "cellSpan" {
                    // Parse colspan and rowspan attributes
                    for attr in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(attr.key.as_ref());
                        let value = String::from_utf8_lossy(&attr.value);
                        match key.as_ref() {
                            "colSpan" => {
                                current_cell.col_span = value.parse().unwrap_or(1);
                            }
                            "rowSpan" => {
                                current_cell.row_span = value.parse().unwrap_or(1);
                            }
                            _ => {}
                        }
                    }
                } else if local_name.ends_with(":cellAddr") || local_name == "cellAddr" {
                    // Parse cell address (actual column and row position)
                    for attr in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(attr.key.as_ref());
                        let value = String::from_utf8_lossy(&attr.value);
                        match key.as_ref() {
                            "colAddr" => {
                                current_cell.col_addr = Some(value.parse().unwrap_or(0));
                            }
                            "rowAddr" => {
                                current_cell.row_addr = Some(value.parse().unwrap_or(0));
                            }
                            _ => {}
                        }
                    }
                } else if local_name.ends_with(":img") || local_name == "img" {
                    // Parse image element - extract binaryItemIDRef
                    // <hc:img binaryItemIDRef="image1" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/>
                    for attr in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(attr.key.as_ref());
                        let value = String::from_utf8_lossy(&attr.value);
                        if key == "binaryItemIDRef" {
                            current_image_ref = Some(value.to_string());
                        }
                    }
                }
            }
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let local_name = String::from_utf8_lossy(name.as_ref());

                match local_name.as_ref() {
                    s if s.ends_with(":p") || s == "p" => {
                        para_depth += 1;
                        if !in_table && para_depth == 1 {
                            current_text.clear();
                        }
                    }
                    s if s.ends_with(":t") || s == "t" => {
                        in_text = true;
                    }
                    s if s.ends_with(":tbl") || s == "tbl" => {
                        in_table = true;
                        table_rows.clear();
                        table_caption.clear();
                    }
                    s if s.ends_with(":caption") || s == "caption" => {
                        in_caption = true;
                    }
                    s if s.ends_with(":tr") || s == "tr" => {
                        current_row.clear();
                    }
                    s if s.ends_with(":tc") || s == "tc" => {
                        in_cell = true;
                        current_cell = HwpxCell::default();
                    }
                    s if s.ends_with(":pic") || s == "pic" => {
                        _in_picture = true;
                        current_image_ref = None;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_text {
                    let text = e.unescape().unwrap_or_default().to_string();
                    if in_table && in_caption {
                        // Text inside table caption
                        table_caption.push_str(&text);
                    } else if in_table && in_cell {
                        current_cell.text.push_str(&text);
                    } else if !in_table {
                        current_text.push_str(&text);
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let local_name = String::from_utf8_lossy(name.as_ref());

                match local_name.as_ref() {
                    s if s.ends_with(":p") || s == "p" => {
                        if para_depth == 1 && !in_table && !current_text.is_empty() {
                            paragraphs.push(create_paragraph(&current_text));
                            current_text.clear();
                        }
                        // Add newline between paragraphs inside cells
                        if in_cell && !current_cell.text.is_empty() {
                            current_cell.text.push('\n');
                        }
                        // Add newline between nested paragraphs (e.g., in drawText/container)
                        // This ensures proper line breaks in TOC and other nested structures
                        if para_depth > 1 && !in_table && !current_text.is_empty() {
                            current_text.push('\n');
                        }
                        para_depth = para_depth.saturating_sub(1);
                    }
                    s if s.ends_with(":t") || s == "t" => {
                        in_text = false;
                    }
                    s if s.ends_with(":caption") || s == "caption" => {
                        in_caption = false;
                    }
                    s if s.ends_with(":tbl") || s == "tbl" => {
                        // Add caption as a paragraph before the table
                        let caption_trimmed = table_caption.trim();
                        if !caption_trimmed.is_empty() {
                            paragraphs.push(create_paragraph(caption_trimmed));
                        }
                        if !table_rows.is_empty() {
                            paragraphs.push(create_table_paragraph_with_spans(&table_rows));
                        }
                        in_table = false;
                        table_caption.clear();
                    }
                    s if s.ends_with(":tr") || s == "tr" => {
                        if !current_row.is_empty() {
                            table_rows.push(current_row.clone());
                        }
                    }
                    s if s.ends_with(":tc") || s == "tc" => {
                        // Trim trailing newline from cell text
                        current_cell.text = current_cell.text.trim_end_matches('\n').to_string();
                        current_row.push(current_cell.clone());
                        in_cell = false;
                    }
                    s if s.ends_with(":pic") || s == "pic" => {
                        // Create image paragraph when picture element ends
                        if let Some(ref image_ref) = current_image_ref {
                            paragraphs.push(create_image_paragraph(image_ref));
                        }
                        _in_picture = false;
                        current_image_ref = None;
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
    }];

    records.push(ParagraphRecord::ParaText {
        text: text.to_string(),
        runs,
        control_char_positions: vec![],
        inline_control_params: vec![],
    });

    Paragraph {
        para_header,
        records,
    }
}

/// Create a paragraph containing a table with proper colspan/rowspan
fn create_table_paragraph_with_spans(rows: &[Vec<HwpxCell>]) -> Paragraph {
    let row_count = rows.len() as UINT16;

    // Calculate actual column count from maximum (col_addr + col_span) across all cells
    // This handles cells with explicit addresses correctly
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

    let mut cells = Vec::new();

    for (row_idx, row) in rows.iter().enumerate() {
        // Track calculated col_address for cells without explicit address
        let mut calc_col_address: u16 = 0;

        for cell_data in row.iter() {
            // Use explicit address if available, otherwise use calculated
            let col_address = cell_data.col_addr.unwrap_or(calc_col_address);
            let row_address = cell_data.row_addr.unwrap_or(row_idx as u16);

            let cell = TableCell {
                list_header: ListHeader {
                    paragraph_count: 1,
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
                    width: HWPUNIT(5000),  // Default width
                    height: HWPUNIT(1000), // Default height
                    left_margin: 0,
                    right_margin: 0,
                    top_margin: 0,
                    bottom_margin: 0,
                    border_fill_id: 0,
                },
                paragraphs: vec![create_paragraph(&cell_data.text)],
            };
            cells.push(cell);

            // Update calculated col_address based on actual position + colspan
            calc_col_address = col_address + cell_data.col_span;
        }
    }

    let table = Table {
        attributes: table_attributes,
        cells,
    };

    // Create paragraph with table
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
