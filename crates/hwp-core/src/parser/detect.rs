/// File format detection for HWP/HWPX files
///
/// HWP 5.0 uses CFB (Compound File Binary) format with magic bytes: D0 CF 11 E0 A1 B1 1A E1
/// HWPX uses ZIP format with magic bytes: 50 4B 03 04 (PK..)

/// Supported file formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    /// HWP 5.0 format (CFB-based, binary)
    Hwp5,
    /// HWPX format (ZIP-based, XML)
    Hwpx,
    /// Unknown or unsupported format
    Unknown,
}

/// CFB (Compound File Binary) magic bytes
/// Used by HWP 5.0 files
const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// ZIP magic bytes (PK..)
/// Used by HWPX files
const ZIP_MAGIC: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];

/// Detect file format from byte array
///
/// # Arguments
/// * `data` - Byte array containing the file data
///
/// # Returns
/// Detected file format
///
/// # Examples
/// ```
/// use hwp_core::parser::detect_format;
///
/// // HWP 5.0 file (CFB magic bytes)
/// let hwp_data = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// assert!(matches!(detect_format(&hwp_data), hwp_core::parser::FileFormat::Hwp5));
///
/// // HWPX file (ZIP magic bytes)
/// let hwpx_data = [0x50, 0x4B, 0x03, 0x04];
/// assert!(matches!(detect_format(&hwpx_data), hwp_core::parser::FileFormat::Hwpx));
/// ```
pub fn detect_format(data: &[u8]) -> FileFormat {
    // Check for CFB magic bytes (HWP 5.0)
    if data.len() >= CFB_MAGIC.len() && data[..CFB_MAGIC.len()] == CFB_MAGIC {
        return FileFormat::Hwp5;
    }

    // Check for ZIP magic bytes (HWPX)
    if data.len() >= ZIP_MAGIC.len() && data[..ZIP_MAGIC.len()] == ZIP_MAGIC {
        return FileFormat::Hwpx;
    }

    FileFormat::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_hwp5() {
        let data = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0x00, 0x00];
        assert_eq!(detect_format(&data), FileFormat::Hwp5);
    }

    #[test]
    fn test_detect_hwpx() {
        let data = [0x50, 0x4B, 0x03, 0x04, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(detect_format(&data), FileFormat::Hwpx);
    }

    #[test]
    fn test_detect_unknown() {
        let data = [0x00, 0x00, 0x00, 0x00];
        assert_eq!(detect_format(&data), FileFormat::Unknown);
    }

    #[test]
    fn test_detect_empty() {
        let data: [u8; 0] = [];
        assert_eq!(detect_format(&data), FileFormat::Unknown);
    }

    #[test]
    fn test_detect_too_short() {
        let data = [0xD0, 0xCF];
        assert_eq!(detect_format(&data), FileFormat::Unknown);
    }
}
