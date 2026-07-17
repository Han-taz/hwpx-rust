/// HWPX ZIP container handling
///
/// HWPX files are ZIP archives containing XML files and binary data.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use unicode_normalization::UnicodeNormalization;
use zip::ZipArchive;

use crate::error::HwpError;

/// Maximum uncompressed size for a single HWPX archive entry.
///
/// HWPX documents may embed images, so this is intentionally generous while still
/// preventing malicious ZIP metadata from driving unbounded allocation.
pub(crate) const MAX_HWPX_ENTRY_UNCOMPRESSED_SIZE: u64 = 128 * 1024 * 1024;
pub(crate) const MAX_HWPX_ENTRY_COMPRESSION_RATIO: u64 = 1_000;
pub(crate) const MAX_HWPX_ARCHIVE_SIZE: u64 = 512 * 1024 * 1024;
pub(crate) const MAX_HWPX_TOTAL_UNCOMPRESSED_SIZE: u64 = 512 * 1024 * 1024;
pub(crate) const MAX_HWPX_ARCHIVE_ENTRIES: u64 = 8_192;
pub(crate) const MAX_HWPX_ENTRY_PATH_BYTES: usize = 4 * 1024;
const MAX_HWPX_MIMETYPE_SIZE: u64 = 4 * 1024;

/// HWPX container wrapper around ZIP archive
pub struct HwpxContainer<'a> {
    archive: ZipArchive<Cursor<&'a [u8]>>,
    file_list: Vec<String>, // cached at creation
    file_lookup: BTreeMap<String, String>,
}

impl<'a> HwpxContainer<'a> {
    /// Open HWPX container from byte array
    pub fn open(data: &'a [u8]) -> Result<Self, HwpError> {
        validate_archive_byte_size(data.len() as u64)?;

        let cursor = Cursor::new(data);
        let mut archive =
            ZipArchive::new(cursor).map_err(|e| HwpError::ZipParseError(e.to_string()))?;

        validate_archive_limits(&mut archive)?;

        let file_list: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
        let file_lookup = file_list
            .iter()
            .map(|path| (canonical_entry_path_key(path), path.clone()))
            .collect();

        Ok(Self {
            archive,
            file_list,
            file_lookup,
        })
    }

    /// Verify mimetype file contains "application/hwp+zip" or similar
    pub fn verify_mimetype(&mut self) -> Result<(), HwpError> {
        match self.read_file_string_with_limit(
            "mimetype",
            MAX_HWPX_MIMETYPE_SIZE,
            "HWPX mimetype byte size",
        ) {
            Ok(mimetype) => validate_mimetype(&mimetype),
            Err(HwpError::HwpxFileNotFound { path }) if path == "mimetype" => {
                // mimetype file is optional in some HWPX implementations
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    /// Read a file from the archive
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>, HwpError> {
        self.read_file_with_limit(
            path,
            MAX_HWPX_ENTRY_UNCOMPRESSED_SIZE,
            "HWPX ZIP entry uncompressed size",
        )
    }

    pub(crate) fn read_file_with_limit(
        &mut self,
        path: &str,
        max_uncompressed_size: u64,
        resource: &'static str,
    ) -> Result<Vec<u8>, HwpError> {
        let archive_path = self
            .entry_name_for(path)
            .ok_or_else(|| HwpError::HwpxFileNotFound {
                path: path.to_string(),
            })?
            .to_string();
        let file = self
            .archive
            .by_name(&archive_path)
            .map_err(|_| HwpError::HwpxFileNotFound {
                path: path.to_string(),
            })?;

        let declared_size = file.size();
        validate_entry_size_with_limit(
            &archive_path,
            declared_size,
            max_uncompressed_size,
            resource,
        )?;

        let capacity = declared_size.min(max_uncompressed_size) as usize;
        let mut buffer = Vec::with_capacity(capacity);
        let mut limited_file = file.take(max_uncompressed_size.saturating_add(1));
        limited_file
            .read_to_end(&mut buffer)
            .map_err(|e| HwpError::Io(e.to_string()))?;

        if buffer.len() as u64 > max_uncompressed_size {
            return Err(HwpError::ResourceLimitExceeded {
                resource,
                path: archive_path,
                limit: max_uncompressed_size,
                actual: buffer.len() as u64,
            });
        }

        Ok(buffer)
    }

    /// Read a file as UTF-8 string
    pub fn read_file_string(&mut self, path: &str) -> Result<String, HwpError> {
        let data = self.read_file(path)?;
        Self::decode_utf8_file(path, data)
    }

    pub(crate) fn read_file_string_with_limit(
        &mut self,
        path: &str,
        max_uncompressed_size: u64,
        resource: &'static str,
    ) -> Result<String, HwpError> {
        let data = self.read_file_with_limit(path, max_uncompressed_size, resource)?;
        Self::decode_utf8_file(path, data)
    }

    fn decode_utf8_file(path: &str, data: Vec<u8>) -> Result<String, HwpError> {
        String::from_utf8(data).map_err(|e| HwpError::EncodingError {
            reason: format!("{path}: {e}"),
        })
    }

    /// List all files in a directory
    pub fn list_files(&self, prefix: &str) -> Vec<String> {
        self.file_list
            .iter()
            .filter(|name| name.starts_with(prefix))
            .cloned()
            .collect()
    }

    /// Check if a file exists
    pub fn file_exists(&self, path: &str) -> bool {
        self.entry_name_for(path).is_some()
    }

    /// Get the list of section files (section0.xml, section1.xml, etc.)
    pub fn get_section_files(&self) -> Vec<String> {
        self.get_section_file_entries()
            .into_iter()
            .map(|(_, path)| path)
            .collect()
    }

    pub(crate) fn get_section_file_entries(&self) -> Vec<(usize, String)> {
        let mut sections: Vec<(usize, String)> = self
            .file_list
            .iter()
            .filter_map(|name| extract_section_number(name).map(|number| (number, name.clone())))
            .collect();

        sections.sort_by_key(|(number, _)| *number);
        sections
    }

    /// Get the list of binary data files
    pub fn get_bindata_files(&self) -> Vec<String> {
        self.file_list
            .iter()
            .filter(|name| is_direct_bindata_file(name))
            .cloned()
            .collect()
    }

    fn entry_name_for(&self, path: &str) -> Option<&str> {
        self.file_lookup
            .get(&canonical_entry_path_key(path))
            .map(String::as_str)
    }
}

/// Extract section number from filename (e.g., "Contents/section0.xml" -> 0)
pub(crate) fn extract_section_number(path: &str) -> Option<usize> {
    let filename = path.split('/').next_back()?;
    if path.rsplit_once('/').map(|(dir, _)| dir) != Some("Contents") {
        return None;
    }
    let num_str = filename.strip_prefix("section")?.strip_suffix(".xml")?;
    if num_str.is_empty() || !num_str.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    num_str.parse().ok()
}

fn is_direct_bindata_file(path: &str) -> bool {
    let Some(file_name) = path.strip_prefix("BinData/") else {
        return false;
    };

    !file_name.is_empty() && !file_name.contains('/')
}

fn validate_mimetype(mimetype: &str) -> Result<(), HwpError> {
    let trimmed = mimetype.trim();
    let media_type = trimmed
        .split_once(';')
        .map(|(media_type, _)| media_type)
        .unwrap_or(trimmed)
        .trim()
        .to_ascii_lowercase();

    let is_supported = matches!(
        media_type.as_str(),
        "application/hwp+zip"
            | "application/x-hwp+zip"
            | "application/owpml"
            | "application/x-owpml"
            | "application/vnd.hancom.hwpx"
    );

    if is_supported {
        return Ok(());
    }

    Err(HwpError::InvalidHwpxStructure {
        reason: format!(
            "Invalid mimetype: expected HWPX media type, got '{}'",
            trimmed.escape_debug()
        ),
    })
}

fn validate_entry_size(path: &str, uncompressed_size: u64) -> Result<(), HwpError> {
    validate_entry_size_with_limit(
        path,
        uncompressed_size,
        MAX_HWPX_ENTRY_UNCOMPRESSED_SIZE,
        "HWPX ZIP entry uncompressed size",
    )
}

fn validate_entry_size_with_limit(
    path: &str,
    uncompressed_size: u64,
    limit: u64,
    resource: &'static str,
) -> Result<(), HwpError> {
    if uncompressed_size > limit {
        return Err(HwpError::ResourceLimitExceeded {
            resource,
            path: path.to_string(),
            limit,
            actual: uncompressed_size,
        });
    }

    Ok(())
}

fn validate_entry_compression_ratio(
    path: &str,
    compressed_size: u64,
    uncompressed_size: u64,
) -> Result<(), HwpError> {
    if uncompressed_size == 0 {
        return Ok(());
    }

    let ratio = (uncompressed_size - 1)
        .checked_div(compressed_size)
        .map_or(u64::MAX, |ratio| ratio + 1);

    if ratio > MAX_HWPX_ENTRY_COMPRESSION_RATIO {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX ZIP entry compression ratio",
            path: path.to_string(),
            limit: MAX_HWPX_ENTRY_COMPRESSION_RATIO,
            actual: ratio,
        });
    }

    Ok(())
}

fn validate_entry_compression_method(
    path: &str,
    compression_method: zip::CompressionMethod,
) -> Result<(), HwpError> {
    match compression_method {
        zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated => Ok(()),
        other => Err(HwpError::InvalidHwpxStructure {
            reason: format!("unsupported HWPX ZIP compression method for {path}: {other}"),
        }),
    }
}

fn validate_entry_encryption(path: &str, encrypted: bool) -> Result<(), HwpError> {
    if encrypted {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("encrypted HWPX ZIP entries are not supported: {path}"),
        });
    }

    Ok(())
}

fn validate_archive_entry_count(entry_count: u64) -> Result<(), HwpError> {
    if entry_count > MAX_HWPX_ARCHIVE_ENTRIES {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX ZIP entry count",
            path: "<archive>".to_string(),
            limit: MAX_HWPX_ARCHIVE_ENTRIES,
            actual: entry_count,
        });
    }

    Ok(())
}

fn validate_archive_byte_size(byte_size: u64) -> Result<(), HwpError> {
    if byte_size > MAX_HWPX_ARCHIVE_SIZE {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX ZIP archive byte size",
            path: "<archive>".to_string(),
            limit: MAX_HWPX_ARCHIVE_SIZE,
            actual: byte_size,
        });
    }

    Ok(())
}

fn validate_archive_total_uncompressed_size(path: &str, total_size: u64) -> Result<(), HwpError> {
    if total_size > MAX_HWPX_TOTAL_UNCOMPRESSED_SIZE {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX ZIP total uncompressed size",
            path: path.to_string(),
            limit: MAX_HWPX_TOTAL_UNCOMPRESSED_SIZE,
            actual: total_size,
        });
    }

    Ok(())
}

fn validate_entry_path(path: &str) -> Result<(), HwpError> {
    if path.len() > MAX_HWPX_ENTRY_PATH_BYTES {
        return Err(HwpError::ResourceLimitExceeded {
            resource: "HWPX ZIP entry path bytes",
            path: "<entry>".to_string(),
            limit: MAX_HWPX_ENTRY_PATH_BYTES as u64,
            actual: path.len() as u64,
        });
    }

    let normalized = path.strip_suffix('/').unwrap_or(path);
    let is_safe = !normalized.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && !path.chars().any(char::is_control)
        && normalized
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..");

    if !is_safe {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("unsafe HWPX ZIP entry path: {}", path.escape_debug()),
        });
    }

    validate_section_entry_index(path)?;

    Ok(())
}

fn validate_section_entry_index(path: &str) -> Result<(), HwpError> {
    let Some(index_part) = path
        .strip_prefix("Contents/section")
        .and_then(|suffix| suffix.strip_suffix(".xml"))
    else {
        return Ok(());
    };

    if index_part.is_empty() || !index_part.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(());
    }

    if index_part.parse::<usize>().is_err() {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("HWPX section entry index is too large: {path}"),
        });
    }

    Ok(())
}

fn validate_unique_entry_path(
    seen_paths: &mut BTreeSet<String>,
    path: &str,
) -> Result<(), HwpError> {
    let canonical_path = canonical_entry_path_key(path);
    if !seen_paths.insert(canonical_path) {
        return Err(HwpError::InvalidHwpxStructure {
            reason: format!("duplicate HWPX ZIP entry path: {path}"),
        });
    }

    Ok(())
}

fn canonical_entry_path_key(path: &str) -> String {
    path.trim_end_matches('/').nfc().collect()
}

fn validate_archive_limits(archive: &mut ZipArchive<Cursor<&[u8]>>) -> Result<(), HwpError> {
    validate_archive_entry_count(archive.len() as u64)?;

    let mut total_uncompressed_size = 0_u64;
    let mut seen_paths = BTreeSet::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index_raw(index)
            .map_err(|e| HwpError::ZipParseError(e.to_string()))?;
        let name = file.name().to_string();
        let size = file.size();
        let compressed_size = file.compressed_size();
        let compression_method = file.compression();
        let encrypted = file.encrypted();

        validate_entry_path(&name)?;
        validate_unique_entry_path(&mut seen_paths, &name)?;
        validate_entry_encryption(&name, encrypted)?;
        validate_entry_compression_method(&name, compression_method)?;
        validate_entry_size(&name, size)?;
        validate_entry_compression_ratio(&name, compressed_size, size)?;
        total_uncompressed_size = total_uncompressed_size.saturating_add(size);
        validate_archive_total_uncompressed_size("<archive>", total_uncompressed_size)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn zip_with_bindata_directories() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        zip.add_directory("BinData/", SimpleFileOptions::default())
            .unwrap();
        zip.start_file("BinData/image1.png", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"png-data").unwrap();
        zip.add_directory("BinData/nested/", SimpleFileOptions::default())
            .unwrap();
        zip.start_file("BinData/nested/image2.png", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"nested-png-data").unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn patch_zip_compression_method_code(data: &mut [u8], method_code: u16) -> usize {
        if data.len() < 4 {
            return 0;
        }

        let method_bytes = method_code.to_le_bytes();
        let mut patched = 0;
        for offset in 0..=data.len() - 4 {
            if data[offset..offset + 4] == [0x50, 0x4b, 0x03, 0x04] && offset + 10 <= data.len() {
                data[offset + 8..offset + 10].copy_from_slice(&method_bytes);
                patched += 1;
            }
            if data[offset..offset + 4] == [0x50, 0x4b, 0x01, 0x02] && offset + 12 <= data.len() {
                data[offset + 10..offset + 12].copy_from_slice(&method_bytes);
                patched += 1;
            }
        }
        patched
    }

    fn patch_zip_general_purpose_flags(data: &mut [u8], flags_to_set: u16) -> usize {
        if data.len() < 4 {
            return 0;
        }

        let mut patched = 0;
        for offset in 0..=data.len() - 4 {
            if data[offset..offset + 4] == [0x50, 0x4b, 0x03, 0x04] && offset + 8 <= data.len() {
                let flags = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
                data[offset + 6..offset + 8].copy_from_slice(&(flags | flags_to_set).to_le_bytes());
                patched += 1;
            }
            if data[offset..offset + 4] == [0x50, 0x4b, 0x01, 0x02] && offset + 10 <= data.len() {
                let flags = u16::from_le_bytes([data[offset + 8], data[offset + 9]]);
                data[offset + 8..offset + 10]
                    .copy_from_slice(&(flags | flags_to_set).to_le_bytes());
                patched += 1;
            }
        }
        patched
    }

    #[test]
    fn test_extract_section_number() {
        assert_eq!(extract_section_number("Contents/section0.xml"), Some(0));
        assert_eq!(extract_section_number("Contents/section10.xml"), Some(10));
        assert_eq!(extract_section_number("Contents/header.xml"), None);
    }

    #[test]
    fn section_file_listing_uses_only_numeric_section_names() {
        let data = zip_with_files(&[
            ("Contents/section10.xml", b""),
            ("Contents/sectiondraft.xml", b""),
            ("Contents/section2.xml", b""),
            ("Contents/section001.xml", b""),
        ]);
        let container = HwpxContainer::open(&data).expect("valid test archive should open");

        assert_eq!(
            container.get_section_files(),
            vec![
                "Contents/section001.xml".to_string(),
                "Contents/section2.xml".to_string(),
                "Contents/section10.xml".to_string(),
            ]
        );
    }

    #[test]
    fn section_file_entries_include_parsed_numbers_and_sorted_paths() {
        let data = zip_with_files(&[
            ("Contents/section10.xml", b""),
            ("Contents/sectiondraft.xml", b""),
            ("Contents/section2.xml", b""),
            ("Contents/section001.xml", b""),
        ]);
        let container = HwpxContainer::open(&data).expect("valid test archive should open");

        assert_eq!(
            container.get_section_file_entries(),
            vec![
                (1, "Contents/section001.xml".to_string()),
                (2, "Contents/section2.xml".to_string()),
                (10, "Contents/section10.xml".to_string()),
            ]
        );
    }

    #[test]
    fn open_rejects_section_entry_number_over_usize_range() {
        let data = zip_with_files(&[(
            "Contents/section999999999999999999999999999999999999.xml",
            b"",
        )]);

        let err = match HwpxContainer::open(&data) {
            Ok(_) => {
                panic!("numeric section entry names over usize range should be rejected");
            }
            Err(err) => err,
        };

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("HWPX section entry index is too large")
                    && reason.contains(
                        "Contents/section999999999999999999999999999999999999.xml"
                    )
        ));
    }

    #[test]
    fn bindata_file_listing_excludes_directories_and_nested_entries() {
        let data = zip_with_bindata_directories();
        let container = HwpxContainer::open(&data).expect("valid test archive should open");

        assert_eq!(
            container.get_bindata_files(),
            vec!["BinData/image1.png".to_string()]
        );
    }

    #[test]
    fn read_file_resolves_unicode_normalization_equivalent_entry_name() {
        let data = zip_with_files(&[("BinData/cafe\u{301}.png", b"png-data")]);
        let mut container = HwpxContainer::open(&data).expect("valid test archive should open");

        assert!(
            container.file_exists("BinData/caf\u{e9}.png"),
            "file_exists should use the same Unicode-normalized key as duplicate detection"
        );
        assert_eq!(
            container
                .read_file("BinData/caf\u{e9}.png")
                .expect("read_file should resolve Unicode-normalization equivalent entry names"),
            b"png-data"
        );
    }

    #[test]
    fn verify_mimetype_rejects_substring_matches() {
        let data = zip_with_files(&[("mimetype", b"not-hwp-but-contains-hwp")]);
        let mut container = HwpxContainer::open(&data).expect("valid test archive should open");

        let err = container
            .verify_mimetype()
            .expect_err("mimetype should be validated by exact media type");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("Invalid mimetype")
                    && reason.contains("not-hwp-but-contains-hwp")
        ));
    }

    #[test]
    fn verify_mimetype_rejects_oversized_mimetype_entry() {
        let oversized_mimetype = vec![b'a'; 4097];
        let data = zip_with_files(&[("mimetype", &oversized_mimetype)]);
        let mut container = HwpxContainer::open(&data).expect("valid test archive should open");

        let err = container
            .verify_mimetype()
            .expect_err("oversized mimetype entries should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX mimetype byte size"
                && path == "mimetype"
                && limit == 4096
                && actual == 4097
        ));
    }

    #[test]
    fn verify_mimetype_accepts_known_hwpx_media_types() {
        for mimetype in [
            "application/hwp+zip",
            "application/x-hwp+zip",
            "application/owpml",
            "application/x-owpml",
            "application/vnd.hancom.hwpx",
            "Application/HWP+ZIP; charset=utf-8",
        ] {
            let data = zip_with_files(&[("mimetype", mimetype.as_bytes())]);
            let mut container = HwpxContainer::open(&data).expect("valid test archive should open");

            container
                .verify_mimetype()
                .expect("known HWPX mimetype should be accepted");
        }
    }

    #[test]
    fn verify_mimetype_rejects_invalid_utf8_with_path_context() {
        let data = zip_with_files(&[("mimetype", b"\xff")]);
        let mut container = HwpxContainer::open(&data).expect("valid test archive should open");

        let err = container
            .verify_mimetype()
            .expect_err("invalid UTF-8 mimetype should be rejected");

        assert!(matches!(
            err,
            HwpError::EncodingError { reason }
                if reason.contains("mimetype")
                    && reason.contains("invalid utf-8")
        ));
    }

    #[test]
    fn rejects_entry_declared_larger_than_hwpx_limit() {
        let err = validate_entry_size(
            "Contents/section0.xml",
            MAX_HWPX_ENTRY_UNCOMPRESSED_SIZE + 1,
        )
        .expect_err("oversized HWPX entry should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX ZIP entry uncompressed size"
                && path == "Contents/section0.xml"
                && limit == MAX_HWPX_ENTRY_UNCOMPRESSED_SIZE
                && actual == MAX_HWPX_ENTRY_UNCOMPRESSED_SIZE + 1
        ));
    }

    #[test]
    fn accepts_entry_declared_at_hwpx_limit() {
        validate_entry_size("Contents/header.xml", MAX_HWPX_ENTRY_UNCOMPRESSED_SIZE)
            .expect("entry at exact HWPX limit should be accepted");
    }

    #[test]
    fn rejects_archive_with_too_many_entries() {
        let err = validate_archive_entry_count(MAX_HWPX_ARCHIVE_ENTRIES + 1)
            .expect_err("archive with too many entries should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX ZIP entry count"
                && path == "<archive>"
                && limit == MAX_HWPX_ARCHIVE_ENTRIES
                && actual == MAX_HWPX_ARCHIVE_ENTRIES + 1
        ));
    }

    #[test]
    fn rejects_archive_bytes_larger_than_hwpx_limit() {
        let err = validate_archive_byte_size(MAX_HWPX_ARCHIVE_SIZE + 1)
            .expect_err("oversized HWPX archive bytes should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX ZIP archive byte size"
                && path == "<archive>"
                && limit == MAX_HWPX_ARCHIVE_SIZE
                && actual == MAX_HWPX_ARCHIVE_SIZE + 1
        ));
    }

    #[test]
    fn accepts_archive_bytes_at_hwpx_limit() {
        validate_archive_byte_size(MAX_HWPX_ARCHIVE_SIZE)
            .expect("archive bytes at exact HWPX limit should be accepted");
    }

    #[test]
    fn rejects_entry_with_excessive_compression_ratio() {
        let err = validate_entry_compression_ratio(
            "Contents/section0.xml",
            1,
            MAX_HWPX_ENTRY_COMPRESSION_RATIO + 1,
        )
        .expect_err("entry compression ratio over HWPX limit should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX ZIP entry compression ratio"
                && path == "Contents/section0.xml"
                && limit == MAX_HWPX_ENTRY_COMPRESSION_RATIO
                && actual == MAX_HWPX_ENTRY_COMPRESSION_RATIO + 1
        ));
    }

    #[test]
    fn accepts_empty_entry_with_zero_compressed_size() {
        validate_entry_compression_ratio("Contents/empty.xml", 0, 0)
            .expect("empty entries with zero compressed size should be accepted");
    }

    #[test]
    fn rejects_archive_entry_with_unsupported_compression_method() {
        let err = validate_entry_compression_method(
            "Contents/section0.xml",
            zip::CompressionMethod::PPMD,
        )
        .expect_err("unsupported HWPX ZIP compression methods should be rejected");

        match err {
            HwpError::InvalidHwpxStructure { reason } => {
                assert!(
                    reason.contains("unsupported HWPX ZIP compression method")
                        && reason.contains("Contents/section0.xml")
                        && reason.contains("98"),
                    "{reason}"
                );
            }
            other => panic!("unexpected error for unsupported compression method: {other:?}"),
        }
    }

    #[test]
    fn accepts_stored_and_deflated_compression_methods() {
        validate_entry_compression_method("Contents/header.xml", zip::CompressionMethod::Stored)
            .expect("stored HWPX ZIP entries should be accepted");
        validate_entry_compression_method(
            "Contents/section0.xml",
            zip::CompressionMethod::Deflated,
        )
        .expect("deflated HWPX ZIP entries should be accepted");
    }

    #[test]
    fn open_rejects_archive_entry_with_unsupported_compression_method() {
        let mut data = zip_with_files(&[("Contents/section0.xml", b"<section/>")]);
        let patched_headers = patch_zip_compression_method_code(&mut data, 98);
        assert_eq!(patched_headers, 2);

        let err = match HwpxContainer::open(&data) {
            Ok(_) => panic!("archives with unsupported compression methods should be rejected"),
            Err(err) => err,
        };

        match err {
            HwpError::InvalidHwpxStructure { reason } => {
                assert!(
                    reason.contains("unsupported HWPX ZIP compression method")
                        && reason.contains("Contents/section0.xml")
                        && reason.contains("98"),
                    "{reason}"
                );
            }
            other => panic!("unexpected error for unsupported compression method: {other:?}"),
        }
    }

    #[test]
    fn rejects_encrypted_archive_entry_metadata() {
        let err = validate_entry_encryption("Contents/section0.xml", true)
            .expect_err("encrypted HWPX ZIP entries should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("encrypted HWPX ZIP entries are not supported")
                    && reason.contains("Contents/section0.xml")
        ));
    }

    #[test]
    fn accepts_unencrypted_archive_entry_metadata() {
        validate_entry_encryption("Contents/header.xml", false)
            .expect("unencrypted HWPX ZIP entries should be accepted");
    }

    #[test]
    fn open_rejects_encrypted_archive_entry() {
        let mut data = zip_with_files(&[("Contents/section0.xml", b"<section/>")]);
        let patched_headers = patch_zip_general_purpose_flags(&mut data, 1);
        assert_eq!(patched_headers, 2);

        let err = match HwpxContainer::open(&data) {
            Ok(_) => panic!("archives with encrypted entries should be rejected"),
            Err(err) => err,
        };

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("encrypted HWPX ZIP entries are not supported")
                    && reason.contains("Contents/section0.xml")
        ));
    }

    #[test]
    fn rejects_archive_with_too_much_total_uncompressed_data() {
        let err = validate_archive_total_uncompressed_size(
            "total.hwpx",
            MAX_HWPX_TOTAL_UNCOMPRESSED_SIZE + 1,
        )
        .expect_err("archive exceeding total uncompressed limit should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX ZIP total uncompressed size"
                && path == "total.hwpx"
                && limit == MAX_HWPX_TOTAL_UNCOMPRESSED_SIZE
                && actual == MAX_HWPX_TOTAL_UNCOMPRESSED_SIZE + 1
        ));
    }

    #[test]
    fn rejects_archive_entry_with_parent_directory_component() {
        let data = zip_with_files(&[("../Contents/section0.xml", b"")]);
        let err = match HwpxContainer::open(&data) {
            Ok(_) => {
                panic!("archive entry names with parent directory components should be rejected")
            }
            Err(err) => err,
        };

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("unsafe HWPX ZIP entry path")
                    && reason.contains("../Contents/section0.xml")
        ));
    }

    #[test]
    fn rejects_archive_entry_with_absolute_path() {
        let data = zip_with_files(&[("/Contents/section0.xml", b"")]);
        let err = match HwpxContainer::open(&data) {
            Ok(_) => panic!("archive entry names with absolute paths should be rejected"),
            Err(err) => err,
        };

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("unsafe HWPX ZIP entry path")
                    && reason.contains("/Contents/section0.xml")
        ));
    }

    #[test]
    fn rejects_archive_entry_with_control_character_path() {
        let err = validate_entry_path("Contents/section0.xml\0.png")
            .expect_err("archive entry names with control characters should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("unsafe HWPX ZIP entry path")
                    && reason.contains("\\0")
        ));
    }

    #[test]
    fn rejects_archive_entry_path_over_byte_limit() {
        let path = format!("Contents/{}.xml", "a".repeat(4097));

        let err = validate_entry_path(&path)
            .expect_err("archive entry names over the byte limit should be rejected");

        assert!(matches!(
            err,
            HwpError::ResourceLimitExceeded {
                resource,
                path,
                limit,
                actual,
            } if resource == "HWPX ZIP entry path bytes"
                && path == "<entry>"
                && limit == 4096
                && actual > 4096
        ));
    }

    #[test]
    fn rejects_archive_entry_with_repeated_trailing_slashes() {
        validate_entry_path("Contents/")
            .expect("single trailing slash directory should be accepted");

        let err = validate_entry_path("Contents//")
            .expect_err("archive entry names with repeated trailing slashes should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("unsafe HWPX ZIP entry path")
                    && reason.contains("Contents//")
        ));
    }

    #[test]
    fn rejects_archive_with_duplicate_entry_names() {
        let mut seen = std::collections::BTreeSet::new();
        validate_unique_entry_path(&mut seen, "Contents/section0.xml")
            .expect("first entry path should be accepted");
        let err = validate_unique_entry_path(&mut seen, "Contents/section0.xml")
            .expect_err("duplicate entry names should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("duplicate HWPX ZIP entry path")
                    && reason.contains("Contents/section0.xml")
        ));
    }

    #[test]
    fn rejects_archive_with_trailing_slash_canonical_duplicate_entry_names() {
        let mut seen = std::collections::BTreeSet::new();
        validate_unique_entry_path(&mut seen, "Contents/")
            .expect("first canonical entry path should be accepted");
        let err = validate_unique_entry_path(&mut seen, "Contents")
            .expect_err("trailing slash variants should be rejected as duplicates");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("duplicate HWPX ZIP entry path")
                    && reason.contains("Contents")
        ));
    }

    #[test]
    fn rejects_archive_with_unicode_normalization_duplicate_entry_names() {
        let mut seen = std::collections::BTreeSet::new();
        validate_unique_entry_path(&mut seen, "BinData/café.png")
            .expect("first canonical Unicode entry path should be accepted");
        let err = validate_unique_entry_path(&mut seen, "BinData/cafe\u{301}.png")
            .expect_err("Unicode-normalization equivalent paths should be rejected");

        assert!(matches!(
            err,
            HwpError::InvalidHwpxStructure { reason }
                if reason.contains("duplicate HWPX ZIP entry path")
                    && reason.contains("BinData/cafe")
        ));
    }
}
