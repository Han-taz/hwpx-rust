/// HWPX Parser module
///
/// HWPX is an XML-based format that uses ZIP compression.
/// It follows the OWPML (Open Word-Processor Markup Language) standard (KS X 6101).
///
/// HWPX file structure:
/// ```text
/// document.hwpx (ZIP)
/// ├── mimetype                    # application/hwp+zip
/// ├── META-INF/
/// │   └── container.xml           # Document structure definition
/// ├── Contents/
/// │   ├── header.xml              # Document settings (styles, fonts)
/// │   ├── content.hpf             # Section list (OPF format)
/// │   └── section0.xml            # Body content
/// ├── BinData/                    # Binary data (images, OLE)
/// └── Preview/                    # Preview images
/// ```
pub mod bindata;
pub mod container;
pub mod header;
pub(crate) mod package;
pub mod section;
pub(crate) mod xml_attr;
pub(crate) mod xml_budget;

use crate::document::HwpDocument;
use crate::error::HwpError;

use container::HwpxContainer;

const MAX_HWPX_PREVIEW_TEXT_SIZE: u64 = 8 * 1024 * 1024;

/// Parse HWPX file from byte array
///
/// # Arguments
/// * `data` - Byte array containing the HWPX file data (ZIP format)
///
/// # Returns
/// Parsed HWP document structure
///
/// # Example
/// ```ignore
/// use hwp_core::parser::hwpx;
///
/// let data = std::fs::read("document.hwpx")?;
/// let document = hwpx::parse(&data)?;
/// println!("Parsed {} sections", document.body_text.sections.len());
/// ```
pub fn parse(data: &[u8]) -> Result<HwpDocument, HwpError> {
    // Open the ZIP container
    let mut container = HwpxContainer::open(data)?;

    // Verify mimetype (optional but recommended)
    container.verify_mimetype()?;

    // Parse file header from version.xml
    let file_header = header::parse_file_header(&mut container)?;

    // Create document with file header
    let mut document = HwpDocument::new(file_header);

    // Parse document info from header.xml
    document.doc_info = header::parse_doc_info(
        &mut container,
        &mut document.warnings,
        &mut document.diagnostics,
    )?;

    // Parse body text from section files
    document.body_text = section::parse_sections(
        &mut container,
        &mut document.warnings,
        &mut document.diagnostics,
    )?;

    // Parse binary data (images, etc.)
    document.bin_data = bindata::parse_bindata(
        &mut container,
        &mut document.warnings,
        &mut document.diagnostics,
    )?;
    bindata::record_missing_referenced_bindata(
        &document.body_text,
        &document.bin_data,
        &mut document.diagnostics,
    );

    // Parse preview text if available
    let has_preview_text = container.file_exists("Preview/PrvText.txt");
    record_missing_optional_part(
        has_preview_text,
        "Preview/PrvText.txt",
        &mut document.diagnostics,
    );
    if has_preview_text {
        match container.read_file_string_with_limit(
            "Preview/PrvText.txt",
            MAX_HWPX_PREVIEW_TEXT_SIZE,
            "HWPX preview text byte size",
        ) {
            Ok(text) => {
                document.preview_text = Some(crate::document::PreviewText { text });
            }
            Err(err) => {
                record_invalid_optional_part(
                    "Preview/PrvText.txt",
                    &err,
                    &mut document.diagnostics,
                );
            }
        }
    }

    // Resolve display texts for compatibility
    document.resolve_display_texts();

    Ok(document)
}

fn record_missing_optional_part(
    exists: bool,
    path: &str,
    diagnostics: &mut crate::diagnostics::DiagnosticReport,
) {
    if !exists {
        diagnostics.push(
            crate::diagnostics::DiagnosticItem::new(
                crate::diagnostics::DiagnosticSeverity::Info,
                crate::diagnostics::DiagnosticCategory::MissingOptionalPart,
                format!("Optional HWPX part {path} was not found"),
            )
            .with_context(
                crate::diagnostics::DiagnosticContext::new()
                    .with_source(path)
                    .with_component("hwpx"),
            ),
        );
    }
}

fn record_invalid_optional_part(
    path: &str,
    error: &HwpError,
    diagnostics: &mut crate::diagnostics::DiagnosticReport,
) {
    let message = match error {
        HwpError::ResourceLimitExceeded { .. } => {
            format!("Optional HWPX part {path} exceeded a parser resource limit and was skipped: {error}")
        }
        _ => {
            format!("Optional HWPX part {path} could not be read and was skipped: {error}")
        }
    };

    diagnostics.push(
        crate::diagnostics::DiagnosticItem::new(
            crate::diagnostics::DiagnosticSeverity::Warning,
            crate::diagnostics::DiagnosticCategory::InvalidValue,
            message,
        )
        .with_context(
            crate::diagnostics::DiagnosticContext::new()
                .with_source(path)
                .with_component("hwpx"),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{DiagnosticCategory, DiagnosticSeverity};
    use std::io::{Cursor, Write};
    use zip::{write::SimpleFileOptions, ZipWriter};

    fn zip_with_files(files: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

        for (path, data) in files {
            zip.start_file(*path, options)
                .expect("test ZIP entry should start");
            zip.write_all(data)
                .expect("test ZIP entry should be writable");
        }

        zip.finish().expect("test ZIP should finish").into_inner()
    }

    fn minimal_hwpx_with_preview(preview_text: &[u8]) -> Vec<u8> {
        zip_with_files(&[
            ("Contents/header.xml", br#"<hh:head/>"#),
            ("Contents/section0.xml", br#"<hs:sec/>"#),
            ("Preview/PrvText.txt", preview_text),
        ])
    }

    #[test]
    fn test_parse_invalid_data() {
        // Not a valid ZIP file
        let result = parse(&[0x00, 0x01, 0x02, 0x03]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_zip() {
        // Minimal valid ZIP file (empty)
        // PK\x03\x04 header + empty central directory
        let minimal_zip: &[u8] = &[
            0x50, 0x4b, 0x05, 0x06, // End of central directory signature
            0x00, 0x00, // Number of this disk
            0x00, 0x00, // Disk where central directory starts
            0x00, 0x00, // Number of central directory records on this disk
            0x00, 0x00, // Total number of central directory records
            0x00, 0x00, 0x00, 0x00, // Size of central directory
            0x00, 0x00, 0x00, 0x00, // Offset of start of central directory
            0x00, 0x00, // Comment length
        ];
        let result = parse(minimal_zip);
        // Should fail because no section files
        assert!(result.is_err());
    }

    #[test]
    fn missing_optional_part_records_info_diagnostic() {
        let mut diagnostics = crate::diagnostics::DiagnosticReport::default();

        record_missing_optional_part(false, "Preview/PrvText.txt", &mut diagnostics);

        assert!(diagnostics.items.iter().any(|item| {
            item.severity == crate::diagnostics::DiagnosticSeverity::Info
                && item.category == crate::diagnostics::DiagnosticCategory::MissingOptionalPart
                && item.context.source.as_deref() == Some("Preview/PrvText.txt")
        }));
    }

    #[test]
    fn invalid_preview_text_records_diagnostic_without_failing_parse() {
        let data = minimal_hwpx_with_preview(b"\xff");

        let document = parse(&data).expect("invalid optional preview text should be skipped");

        assert!(document.preview_text.is_none());
        assert!(document.diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::Warning
                && item.category == DiagnosticCategory::InvalidValue
                && item.context.source.as_deref() == Some("Preview/PrvText.txt")
                && item.context.component.as_deref() == Some("hwpx")
                && item.message.contains("Preview/PrvText.txt")
        }));
    }

    #[test]
    fn oversized_preview_text_is_skipped_with_diagnostic() {
        let oversized_preview = vec![b'a'; (8 * 1024 * 1024) + 1];
        let data = minimal_hwpx_with_preview(&oversized_preview);

        let document = parse(&data).expect("oversized optional preview text should be skipped");

        assert!(document.preview_text.is_none());
        assert!(document.diagnostics.items.iter().any(|item| {
            item.severity == DiagnosticSeverity::Warning
                && item.category == DiagnosticCategory::InvalidValue
                && item.context.source.as_deref() == Some("Preview/PrvText.txt")
                && item.message.contains("resource limit")
        }));
    }
}
