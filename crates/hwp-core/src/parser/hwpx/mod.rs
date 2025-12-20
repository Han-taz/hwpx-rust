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
pub mod section;

use crate::document::HwpDocument;
use crate::error::HwpError;

use container::HwpxContainer;

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
    document.doc_info = header::parse_doc_info(&mut container)?;

    // Parse body text from section files
    document.body_text = section::parse_sections(&mut container)?;

    // Parse binary data (images, etc.)
    document.bin_data = bindata::parse_bindata(&mut container)?;

    // Parse preview text if available
    if container.file_exists("Preview/PrvText.txt") {
        if let Ok(text) = container.read_file_string("Preview/PrvText.txt") {
            document.preview_text = Some(crate::document::PreviewText { text });
        }
    }

    // Resolve display texts for compatibility
    document.resolve_display_texts();

    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
