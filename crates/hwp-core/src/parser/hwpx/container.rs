/// HWPX ZIP container handling
///
/// HWPX files are ZIP archives containing XML files and binary data.
use std::io::{Cursor, Read};
use zip::ZipArchive;

use crate::error::HwpError;

/// HWPX container wrapper around ZIP archive
pub struct HwpxContainer<'a> {
    archive: ZipArchive<Cursor<&'a [u8]>>,
}

impl<'a> HwpxContainer<'a> {
    /// Open HWPX container from byte array
    pub fn open(data: &'a [u8]) -> Result<Self, HwpError> {
        let cursor = Cursor::new(data);
        let archive =
            ZipArchive::new(cursor).map_err(|e| HwpError::ZipParseError(e.to_string()))?;

        Ok(Self { archive })
    }

    /// Verify mimetype file contains "application/hwp+zip" or similar
    pub fn verify_mimetype(&mut self) -> Result<(), HwpError> {
        match self.read_file("mimetype") {
            Ok(content) => {
                let mimetype = String::from_utf8_lossy(&content);
                let trimmed = mimetype.trim();

                // Accept various HWPX mimetype values
                if trimmed.contains("hwp") || trimmed.contains("owpml") {
                    Ok(())
                } else {
                    Err(HwpError::InvalidHwpxStructure {
                        reason: format!(
                            "Invalid mimetype: expected 'application/hwp+zip' or similar, got '{trimmed}'"
                        ),
                    })
                }
            }
            Err(_) => {
                // mimetype file is optional in some HWPX implementations
                Ok(())
            }
        }
    }

    /// Read a file from the archive
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>, HwpError> {
        let mut file = self
            .archive
            .by_name(path)
            .map_err(|_| HwpError::HwpxFileNotFound {
                path: path.to_string(),
            })?;

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .map_err(|e| HwpError::Io(e.to_string()))?;

        Ok(buffer)
    }

    /// Read a file as UTF-8 string
    pub fn read_file_string(&mut self, path: &str) -> Result<String, HwpError> {
        let data = self.read_file(path)?;
        String::from_utf8(data).map_err(|e| HwpError::EncodingError {
            reason: e.to_string(),
        })
    }

    /// List all files in a directory
    pub fn list_files(&self, prefix: &str) -> Vec<String> {
        self.archive
            .file_names()
            .filter(|name| name.starts_with(prefix))
            .map(|s| s.to_string())
            .collect()
    }

    /// Check if a file exists
    pub fn file_exists(&self, path: &str) -> bool {
        self.archive.file_names().any(|name| name == path)
    }

    /// Get the list of section files (section0.xml, section1.xml, etc.)
    pub fn get_section_files(&self) -> Vec<String> {
        let mut sections: Vec<String> = self
            .archive
            .file_names()
            .filter(|name| name.starts_with("Contents/section") && name.ends_with(".xml"))
            .map(|s| s.to_string())
            .collect();

        // Sort by section number
        sections.sort_by(|a, b| {
            let num_a = extract_section_number(a).unwrap_or(0);
            let num_b = extract_section_number(b).unwrap_or(0);
            num_a.cmp(&num_b)
        });

        sections
    }

    /// Get the list of binary data files
    pub fn get_bindata_files(&self) -> Vec<String> {
        self.archive
            .file_names()
            .filter(|name| name.starts_with("BinData/"))
            .map(|s| s.to_string())
            .collect()
    }
}

/// Extract section number from filename (e.g., "Contents/section0.xml" -> 0)
fn extract_section_number(path: &str) -> Option<usize> {
    let filename = path.split('/').next_back()?;
    let num_str = filename.strip_prefix("section")?.strip_suffix(".xml")?;
    num_str.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_section_number() {
        assert_eq!(extract_section_number("Contents/section0.xml"), Some(0));
        assert_eq!(extract_section_number("Contents/section10.xml"), Some(10));
        assert_eq!(extract_section_number("Contents/header.xml"), None);
    }
}
