/// HWPX header.xml parser
///
/// header.xml contains document settings like character shapes, paragraph shapes,
/// styles, fonts, and other document-wide properties.
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::document::docinfo::char_shape::{
    CharShape, CharShapeAttributes, LanguageCharAttributesI8, LanguageCharAttributesU8,
    LanguageFontInfo,
};
use crate::document::{DocInfo, FileHeader};
use crate::error::HwpError;
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
                            if let Ok(v) = String::from_utf8_lossy(&attr.value).parse::<u32>() {
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
pub fn parse_doc_info(container: &mut HwpxContainer) -> Result<DocInfo, HwpError> {
    let content = container.read_file_string("Contents/header.xml")?;

    let mut reader = Reader::from_str(&content);
    reader.config_mut().trim_text(true);

    // Create a basic DocInfo structure
    // In a full implementation, we would parse character shapes, paragraph shapes, etc.
    let mut doc_info = DocInfo::default();

    // Parse the XML and extract relevant information
    // For now, we create a minimal DocInfo that allows the document to be processed
    parse_header_xml_content(&mut reader, &mut doc_info)?;

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
) -> Result<(), HwpError> {
    let mut in_char_properties = false;
    let mut in_para_shapes = false;
    let mut in_face_names = false;

    // Current charPr being parsed
    let mut current_char_shape: Option<CharShape> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let local_name = String::from_utf8_lossy(name.as_ref());

                match local_name.as_ref() {
                    // HWPX uses charProperties/charPr instead of charShapes/charShape
                    s if s.ends_with("charProperties") => in_char_properties = true,
                    s if s.ends_with("paraShapes") || s.ends_with("paraProperties") => {
                        in_para_shapes = true
                    }
                    s if s.ends_with("faceNames") || s.ends_with("fontfaces") => {
                        in_face_names = true
                    }
                    s if s.ends_with("charPr") && in_char_properties => {
                        // Parse charPr element attributes
                        let mut height: i32 = 1000; // Default 10pt
                        let mut text_color: COLORREF = COLORREF(0);

                        for attr in e.attributes().flatten() {
                            let key = String::from_utf8_lossy(attr.key.as_ref());
                            let value = String::from_utf8_lossy(&attr.value);
                            match key.as_ref() {
                                "height" => {
                                    height = value.parse().unwrap_or(1000);
                                }
                                "textColor" => {
                                    text_color = parse_color(&value);
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
                    s if s.ends_with("fontRef") => {
                        // Parse font references for current char shape
                        if let Some(ref mut cs) = current_char_shape {
                            for attr in e.attributes().flatten() {
                                let key = String::from_utf8_lossy(attr.key.as_ref());
                                let value: u16 = String::from_utf8_lossy(&attr.value)
                                    .parse()
                                    .unwrap_or(0);
                                match key.as_ref() {
                                    "hangul" => cs.font_ids.korean = value,
                                    "latin" => cs.font_ids.english = value,
                                    "hanja" => cs.font_ids.chinese = value,
                                    "japanese" => cs.font_ids.japanese = value,
                                    "other" => cs.font_ids.other = value,
                                    "symbol" => cs.font_ids.symbol = value,
                                    "user" => cs.font_ids.user = value,
                                    _ => {}
                                }
                            }
                        }
                    }
                    s if s.ends_with("underline") => {
                        // Parse underline for current char shape
                        if let Some(ref mut cs) = current_char_shape {
                            for attr in e.attributes().flatten() {
                                let key = String::from_utf8_lossy(attr.key.as_ref());
                                let value = String::from_utf8_lossy(&attr.value);
                                match key.as_ref() {
                                    "type" => {
                                        cs.attributes.underline_type = match value.as_ref() {
                                            "BOTTOM" => 1,
                                            "TOP" => 2,
                                            _ => 0, // NONE
                                        };
                                    }
                                    "color" => {
                                        cs.underline_color = parse_color(&value);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    s if s.ends_with("strikeout") => {
                        // Parse strikethrough for current char shape
                        if let Some(ref mut cs) = current_char_shape {
                            for attr in e.attributes().flatten() {
                                let key = String::from_utf8_lossy(attr.key.as_ref());
                                let value = String::from_utf8_lossy(&attr.value);
                                match key.as_ref() {
                                    "shape" => {
                                        // If shape is not "NONE", strikethrough is enabled
                                        cs.attributes.strikethrough =
                                            if value == "NONE" { 0 } else { 1 };
                                    }
                                    "color" => {
                                        cs.strikethrough_color = Some(parse_color(&value));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    s if s.ends_with("bold") => {
                        // Parse bold for current char shape
                        // HWPX uses <hh:bold/> self-closing element to indicate bold
                        if let Some(ref mut cs) = current_char_shape {
                            cs.attributes.bold = true;
                        }
                    }
                    s if s.ends_with("italic") => {
                        // Parse italic for current char shape
                        // HWPX uses <hh:italic/> self-closing element to indicate italic
                        if let Some(ref mut cs) = current_char_shape {
                            cs.attributes.italic = true;
                        }
                    }
                    s if s.ends_with("paraShape") && in_para_shapes => {
                        // Parse paragraph shape - simplified for now
                    }
                    s if (s.ends_with("font") || s.ends_with("faceName")) && in_face_names => {
                        // Parse font face - simplified for now
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let local_name = String::from_utf8_lossy(name.as_ref());

                match local_name.as_ref() {
                    s if s.ends_with("charProperties") => in_char_properties = false,
                    s if s.ends_with("paraShapes") || s.ends_with("paraProperties") => {
                        in_para_shapes = false
                    }
                    s if s.ends_with("faceNames") || s.ends_with("fontfaces") => {
                        in_face_names = false
                    }
                    s if s.ends_with("charPr") => {
                        // Save completed char shape
                        if let Some(cs) = current_char_shape.take() {
                            doc_info.char_shapes.push(cs);
                        }
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
