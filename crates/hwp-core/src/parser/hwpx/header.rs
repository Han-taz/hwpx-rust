/// HWPX header.xml parser
///
/// header.xml contains document settings like character shapes, paragraph shapes,
/// styles, fonts, and other document-wide properties.
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::diagnostics::DiagnosticReport;
use crate::document::docinfo::char_shape::{
    CharShape, CharShapeAttributes, LanguageCharAttributesI8, LanguageCharAttributesU8,
    LanguageFontInfo,
};
use crate::document::docinfo::para_shape::{
    HeaderShapeType, LineDivideUnit, LineSpacingTypeOld, ParaShape, ParaShapeAttributes1,
    ParagraphAlignment, VerticalAlignment,
};
use crate::document::{DocInfo, FileHeader};
use crate::error::{HwpError, ParseWarning, ParseWarnings};
use crate::types::{COLORREF, DWORD};

use super::container::HwpxContainer;

/// Parse header.xml and create FileHeader
pub fn parse_file_header(container: &mut HwpxContainer) -> Result<FileHeader, HwpError> {
    // Try to read version.xml first for version info
    let version = parse_version_xml(container).unwrap_or(0x05010000); // Default to 5.1.0.0

    // Create a FileHeader compatible with HWP 5.0 structure
    Ok(FileHeader {
        signature: "HWP Document File".to_string(),
        version,
        document_flags: 0, // HWPX is not compressed in the same way as HWP
        license_flags: 0,
        encrypt_version: 0,
        kogl_country: 0,
        reserved: vec![0; 207],
    })
}

/// Parse version.xml for OWPML version info
fn parse_version_xml(container: &mut HwpxContainer) -> Result<DWORD, HwpError> {
    let content = container.read_file_string("version.xml")?;

    let mut reader = Reader::from_str(&content);
    reader.config_mut().trim_text(true);

    let mut version: DWORD = 0x05010000; // Default version

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                if e.name().as_ref() == b"opf:version" || e.name().as_ref() == b"version" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"major"
                            || attr.key.as_ref() == b"oversion"
                            || attr.key.as_ref() == b"app-version"
                        {
                            if let Some(v) = std::str::from_utf8(&attr.value)
                                .ok()
                                .and_then(|s| s.parse::<u32>().ok())
                            {
                                // Convert to HWP version format (major.minor.build.revision)
                                version = (v << 24) | 0x00010000;
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(HwpError::XmlParseError(format!(
                    "Error parsing version.xml: {e}"
                )))
            }
            _ => {}
        }
    }

    Ok(version)
}

/// Parse header.xml and create DocInfo
pub fn parse_doc_info(
    container: &mut HwpxContainer,
    warnings: &mut ParseWarnings,
    diagnostics: &mut DiagnosticReport,
) -> Result<DocInfo, HwpError> {
    let content = container.read_file_string("Contents/header.xml")?;

    let mut reader = Reader::from_str(&content);
    reader.config_mut().trim_text(true);

    // Create a basic DocInfo structure
    // In a full implementation, we would parse character shapes, paragraph shapes, etc.
    let mut doc_info = DocInfo::default();

    // Parse the XML and extract relevant information
    // For now, we create a minimal DocInfo that allows the document to be processed
    parse_header_xml_content(&mut reader, &mut doc_info, warnings, diagnostics)?;

    Ok(doc_info)
}

/// Parse hex color string (#RRGGBB) to COLORREF (BGR format)
fn parse_color(color_str: &str) -> COLORREF {
    let color_str = color_str.trim_start_matches('#');
    if color_str == "none" || color_str.is_empty() {
        return COLORREF(0);
    }
    // Parse as RGB and convert to BGR (COLORREF format)
    let rgb = u32::from_str_radix(color_str, 16).unwrap_or(0);
    let r = ((rgb >> 16) & 0xFF) as u8;
    let g = ((rgb >> 8) & 0xFF) as u8;
    let b = (rgb & 0xFF) as u8;
    COLORREF::rgb(r, g, b)
}

/// Parse header.xml content
fn parse_header_xml_content(
    reader: &mut Reader<&[u8]>,
    doc_info: &mut DocInfo,
    warnings: &mut ParseWarnings,
    _diagnostics: &mut DiagnosticReport,
) -> Result<(), HwpError> {
    let mut in_char_properties = false;
    let mut in_para_shapes = false;
    let mut in_face_names = false;
    let mut in_margin = false; // Track if inside <hh:margin> element

    // Current charPr being parsed
    let mut current_char_shape: Option<CharShape> = None;

    // Current paraPr being parsed
    let mut current_para_shape: Option<ParaShape> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local_name = e.name();
                let local_name = local_name.as_ref();

                match local_name {
                    // HWPX uses charProperties/charPr instead of charShapes/charShape
                    s if s.ends_with(b"charProperties") => in_char_properties = true,
                    s if s.ends_with(b"paraShapes") || s.ends_with(b"paraProperties") => {
                        in_para_shapes = true
                    }
                    s if s.ends_with(b"faceNames") || s.ends_with(b"fontfaces") => {
                        in_face_names = true
                    }
                    s if s.ends_with(b"charPr") && in_char_properties => {
                        // Parse charPr element attributes
                        let mut height: i32 = 1000; // Default 10pt
                        let mut text_color: COLORREF = COLORREF(0);

                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"height" => {
                                    height = std::str::from_utf8(&attr.value)
                                        .ok()
                                        .and_then(|s| s.parse().ok())
                                        .unwrap_or_else(|| {
                                            if let Ok(s) = std::str::from_utf8(&attr.value) {
                                                warnings.push(ParseWarning::warning(format!(
                                                    "Invalid charPr height value: {s}"
                                                )));
                                            }
                                            1000
                                        });
                                }
                                b"textColor" => {
                                    if let Ok(s) = std::str::from_utf8(&attr.value) {
                                        text_color = parse_color(s);
                                    }
                                }
                                _ => {}
                            }
                        }

                        // Create new CharShape with parsed values
                        current_char_shape = Some(CharShape {
                            font_ids: LanguageFontInfo {
                                korean: 0,
                                english: 0,
                                chinese: 0,
                                japanese: 0,
                                other: 0,
                                symbol: 0,
                                user: 0,
                            },
                            font_stretch: LanguageCharAttributesU8 {
                                korean: 100,
                                english: 100,
                                chinese: 100,
                                japanese: 100,
                                other: 100,
                                symbol: 100,
                                user: 100,
                            },
                            letter_spacing: LanguageCharAttributesI8 {
                                korean: 0,
                                english: 0,
                                chinese: 0,
                                japanese: 0,
                                other: 0,
                                symbol: 0,
                                user: 0,
                            },
                            relative_size: LanguageCharAttributesU8 {
                                korean: 100,
                                english: 100,
                                chinese: 100,
                                japanese: 100,
                                other: 100,
                                symbol: 100,
                                user: 100,
                            },
                            text_position: LanguageCharAttributesI8 {
                                korean: 0,
                                english: 0,
                                chinese: 0,
                                japanese: 0,
                                other: 0,
                                symbol: 0,
                                user: 0,
                            },
                            base_size: height, // HWPX height is in 1/100 pt
                            attributes: CharShapeAttributes {
                                italic: false,
                                bold: false,
                                underline_type: 0,
                                underline_style: 0,
                                outline_type: 0,
                                shadow_type: 0,
                                emboss: false,
                                engrave: false,
                                superscript: false,
                                subscript: false,
                                strikethrough: 0,
                                emphasis_mark: 0,
                                use_font_spacing: false,
                                strikethrough_style: 0,
                                kerning: false,
                            },
                            shadow_spacing_x: 0,
                            shadow_spacing_y: 0,
                            text_color,
                            underline_color: COLORREF(0),
                            shading_color: COLORREF(0),
                            shadow_color: COLORREF(0),
                            border_fill_id: None,
                            strikethrough_color: None,
                        });
                    }
                    s if s.ends_with(b"fontRef") => {
                        // Parse font references for current char shape
                        if let Some(ref mut cs) = current_char_shape {
                            for attr in e.attributes().flatten() {
                                let value: u16 = std::str::from_utf8(&attr.value)
                                    .ok()
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or_else(|| {
                                        if let Ok(s) = std::str::from_utf8(&attr.value) {
                                            warnings.push(ParseWarning::warning(format!(
                                                "Invalid fontRef attribute value: {s}"
                                            )));
                                        }
                                        0
                                    });
                                match attr.key.as_ref() {
                                    b"hangul" => cs.font_ids.korean = value,
                                    b"latin" => cs.font_ids.english = value,
                                    b"hanja" => cs.font_ids.chinese = value,
                                    b"japanese" => cs.font_ids.japanese = value,
                                    b"other" => cs.font_ids.other = value,
                                    b"symbol" => cs.font_ids.symbol = value,
                                    b"user" => cs.font_ids.user = value,
                                    _ => {}
                                }
                            }
                        }
                    }
                    s if s.ends_with(b"underline") => {
                        // Parse underline for current char shape
                        if let Some(ref mut cs) = current_char_shape {
                            for attr in e.attributes().flatten() {
                                match attr.key.as_ref() {
                                    b"type" => {
                                        cs.attributes.underline_type = match attr.value.as_ref() {
                                            b"BOTTOM" => 1,
                                            b"TOP" => 2,
                                            _ => 0, // NONE
                                        };
                                    }
                                    b"color" => {
                                        if let Ok(s) = std::str::from_utf8(&attr.value) {
                                            cs.underline_color = parse_color(s);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    s if s.ends_with(b"strikeout") => {
                        // Parse strikethrough for current char shape
                        if let Some(ref mut cs) = current_char_shape {
                            for attr in e.attributes().flatten() {
                                match attr.key.as_ref() {
                                    b"shape" => {
                                        // If shape is not "NONE", strikethrough is enabled
                                        cs.attributes.strikethrough =
                                            if attr.value.as_ref() == b"NONE" { 0 } else { 1 };
                                    }
                                    b"color" => {
                                        if let Ok(s) = std::str::from_utf8(&attr.value) {
                                            cs.strikethrough_color = Some(parse_color(s));
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    s if s.ends_with(b"bold") => {
                        // Parse bold for current char shape
                        // HWPX uses <hh:bold/> self-closing element to indicate bold
                        if let Some(ref mut cs) = current_char_shape {
                            cs.attributes.bold = true;
                        }
                    }
                    s if s.ends_with(b"italic") => {
                        // Parse italic for current char shape
                        // HWPX uses <hh:italic/> self-closing element to indicate italic
                        if let Some(ref mut cs) = current_char_shape {
                            cs.attributes.italic = true;
                        }
                    }
                    s if (s.ends_with(b"paraPr") || s.ends_with(b"paraShape"))
                        && in_para_shapes =>
                    {
                        // Create new ParaShape with default values
                        current_para_shape = Some(ParaShape {
                            attributes1: ParaShapeAttributes1 {
                                line_spacing_type_old: LineSpacingTypeOld::ByCharacter,
                                align: ParagraphAlignment::Justify, // Default: justify
                                line_divide_en: LineDivideUnit::Word,
                                line_divide_ko: LineDivideUnit::Word,
                                use_line_grid: false,
                                blank_min_value: 0,
                                protect_orphan_line: false,
                                with_next_paragraph: false,
                                protect_paragraph: false,
                                always_page_break_before: false,
                                vertical_align: VerticalAlignment::Baseline,
                                line_height_matches_font: false,
                                header_shape_type: HeaderShapeType::None,
                                paragraph_level: 0,
                                connect_border: false,
                                ignore_margin: false,
                                tail_shape: false,
                            },
                            left_margin: 0,
                            right_margin: 0,
                            indent: 0,
                            outdent: 0,
                            top_spacing: 0,
                            bottom_spacing: 0,
                            line_spacing_old: 0,
                            tab_def_id: 0,
                            number_bullet_id: 0,
                            border_fill_id: 0,
                            border_spacing_left: 0,
                            border_spacing_right: 0,
                            border_spacing_top: 0,
                            border_spacing_bottom: 0,
                            attributes2: None,
                            attributes3: None,
                            line_spacing: None,
                        });
                    }
                    s if s.ends_with(b":align") && current_para_shape.is_some() => {
                        // Parse <hh:align horizontal="JUSTIFY|LEFT|RIGHT|CENTER" />
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"horizontal" {
                                if let Some(ref mut ps) = current_para_shape {
                                    ps.attributes1.align = match attr.value.as_ref() {
                                        b"LEFT" => ParagraphAlignment::Left,
                                        b"RIGHT" => ParagraphAlignment::Right,
                                        b"CENTER" => ParagraphAlignment::Center,
                                        b"DISTRIBUTE" => ParagraphAlignment::Distribute,
                                        _ => ParagraphAlignment::Justify,
                                    };
                                }
                            }
                        }
                    }
                    s if s.ends_with(b":margin") && current_para_shape.is_some() => {
                        // Enter <hh:margin> element
                        in_margin = true;
                    }
                    s if s.ends_with(b":intent") && in_margin && current_para_shape.is_some() => {
                        // Parse <hc:intent value="N" unit="HWPUNIT" />
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"value" {
                                if let Some(ref mut ps) = current_para_shape {
                                    // HWPUNIT = 1/7200 inch, HWP INT32 = 1/1800 inch
                                    // Convert: value / 4
                                    let hwpunit_value: i32 = std::str::from_utf8(&attr.value)
                                        .ok()
                                        .and_then(|s| s.parse().ok())
                                        .unwrap_or_else(|| {
                                            if let Ok(s) = std::str::from_utf8(&attr.value) {
                                                warnings.push(ParseWarning::warning(format!(
                                                    "Invalid margin intent value: {s}"
                                                )));
                                            }
                                            0
                                        });
                                    ps.indent = hwpunit_value / 4;
                                }
                            }
                        }
                    }
                    s if s.ends_with(b":left") && in_margin && current_para_shape.is_some() => {
                        // Parse <hc:left value="N" unit="HWPUNIT" />
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"value" {
                                if let Some(ref mut ps) = current_para_shape {
                                    let hwpunit_value: i32 = std::str::from_utf8(&attr.value)
                                        .ok()
                                        .and_then(|s| s.parse().ok())
                                        .unwrap_or_else(|| {
                                            if let Ok(s) = std::str::from_utf8(&attr.value) {
                                                warnings.push(ParseWarning::warning(format!(
                                                    "Invalid margin left value: {s}"
                                                )));
                                            }
                                            0
                                        });
                                    ps.left_margin = hwpunit_value / 4;
                                }
                            }
                        }
                    }
                    s if s.ends_with(b":right") && in_margin && current_para_shape.is_some() => {
                        // Parse <hc:right value="N" unit="HWPUNIT" />
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"value" {
                                if let Some(ref mut ps) = current_para_shape {
                                    let hwpunit_value: i32 = std::str::from_utf8(&attr.value)
                                        .ok()
                                        .and_then(|s| s.parse().ok())
                                        .unwrap_or_else(|| {
                                            if let Ok(s) = std::str::from_utf8(&attr.value) {
                                                warnings.push(ParseWarning::warning(format!(
                                                    "Invalid margin right value: {s}"
                                                )));
                                            }
                                            0
                                        });
                                    ps.right_margin = hwpunit_value / 4;
                                }
                            }
                        }
                    }
                    s if (s.ends_with(b"font") || s.ends_with(b"faceName")) && in_face_names => {
                        // Parse font face - simplified for now
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local_name = e.name();
                let local_name = local_name.as_ref();

                match local_name {
                    s if s.ends_with(b"charProperties") => in_char_properties = false,
                    s if s.ends_with(b"paraShapes") || s.ends_with(b"paraProperties") => {
                        in_para_shapes = false
                    }
                    s if s.ends_with(b"faceNames") || s.ends_with(b"fontfaces") => {
                        in_face_names = false
                    }
                    s if s.ends_with(b"charPr") => {
                        // Save completed char shape
                        if let Some(cs) = current_char_shape.take() {
                            doc_info.char_shapes.push(cs);
                        }
                    }
                    s if s.ends_with(b"paraPr") || s.ends_with(b"paraShape") => {
                        // Save completed para shape
                        if let Some(ps) = current_para_shape.take() {
                            doc_info.para_shapes.push(ps);
                        }
                    }
                    s if s.ends_with(b":margin") => {
                        // Exit <hh:margin> element
                        in_margin = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(HwpError::XmlParseError(format!(
                    "Error parsing header.xml: {e}"
                )))
            }
            _ => {}
        }
    }

    // Create default document properties if not set
    if doc_info.document_properties.is_none() {
        doc_info.document_properties = Some(crate::document::DocumentProperties {
            area_count: 1,
            start_number_info: 0,
            page_start_number: 1,
            footnote_start_number: 1,
            endnote_start_number: 1,
            image_start_number: 1,
            table_start_number: 1,
            formula_start_number: 1,
            list_id: 0,
            paragraph_id: 0,
            character_position: 0,
        });
    }

    Ok(())
}
