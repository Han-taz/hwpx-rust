/// HWPX header.xml parser
///
/// header.xml contains document settings like character shapes, paragraph shapes,
/// styles, fonts, and other document-wide properties.
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::document::{DocInfo, FileHeader};
use crate::error::HwpError;
use crate::types::DWORD;

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

/// Parse header.xml content
fn parse_header_xml_content(
    reader: &mut Reader<&[u8]>,
    doc_info: &mut DocInfo,
) -> Result<(), HwpError> {
    let mut in_char_shapes = false;
    let mut in_para_shapes = false;
    let mut in_face_names = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                let local_name = String::from_utf8_lossy(name.as_ref());

                match local_name.as_ref() {
                    s if s.ends_with("charShapes") => in_char_shapes = true,
                    s if s.ends_with("paraShapes") => in_para_shapes = true,
                    s if s.ends_with("faceNames") || s.ends_with("fontfaces") => {
                        in_face_names = true
                    }
                    s if s.ends_with("charShape") && in_char_shapes => {
                        // Parse character shape - simplified for now
                        // In full implementation, parse all attributes
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
                    s if s.ends_with("charShapes") => in_char_shapes = false,
                    s if s.ends_with("paraShapes") => in_para_shapes = false,
                    s if s.ends_with("faceNames") || s.ends_with("fontfaces") => {
                        in_face_names = false
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
