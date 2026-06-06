/// 뷰어 간 공통 유틸리티 함수 / Shared utility functions across viewers
///
/// HTML 뷰어와 Markdown 뷰어에서 공통으로 사용하는 함수들입니다.
/// Functions shared between HTML and Markdown viewers.
use crate::document::{bindata::BinaryDataItem, HwpDocument};
use crate::error::HwpError;
use crate::types::WORD;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Pre-built index for O(1) BinData lookups by binary_data_id.
/// Maps binary_data_id -> file extension (e.g., "jpg", "png").
pub type BinDataIndex = HashMap<WORD, String>;

/// Pre-built lookup for O(1) BinData item access by binary_data_id or HWPX item name.
#[derive(Debug)]
pub struct BinDataItemLookup<'a> {
    by_id: HashMap<WORD, &'a BinaryDataItem>,
    by_name: HashMap<&'a str, &'a BinaryDataItem>,
}

impl<'a> BinDataItemLookup<'a> {
    /// Return a BinData item by binary_data_id.
    pub fn by_id(&self, bindata_id: WORD) -> Option<&'a BinaryDataItem> {
        self.by_id.get(&bindata_id).copied()
    }

    /// Return a BinData item by HWPX binary item name.
    pub fn by_name(&self, name: &str) -> Option<&'a BinaryDataItem> {
        self.by_name.get(name).copied()
    }
}

/// Build a BinData index from the document's docinfo records.
pub fn build_bindata_index(document: &HwpDocument) -> BinDataIndex {
    use crate::document::BinDataRecord;
    document
        .doc_info
        .bin_data
        .iter()
        .filter_map(|record| {
            if let BinDataRecord::Embedding { embedding, .. } = record {
                Some((embedding.binary_data_id, embedding.extension.clone()))
            } else {
                None
            }
        })
        .collect()
}

/// Build a BinData item lookup from the document's BinData entries.
pub fn build_bindata_item_lookup(document: &HwpDocument) -> BinDataItemLookup<'_> {
    let mut by_id = HashMap::with_capacity(document.bin_data.items.len());
    let mut by_name = HashMap::new();

    for item in &document.bin_data.items {
        by_id.entry(item.index).or_insert(item);
        if let Some(name) = item.name.as_deref() {
            by_name.entry(name).or_insert(item);
        }
    }

    BinDataItemLookup { by_id, by_name }
}

/// Get file extension from BinData ID
/// BinData ID에서 파일 확장자 가져오기
pub fn get_extension_from_bindata_id(index: &BinDataIndex, bindata_id: WORD) -> &str {
    index
        .get(&bindata_id)
        .and_then(|ext| safe_bindata_extension(ext))
        .unwrap_or("jpg")
}

/// Get MIME type from BinData ID
/// BinData ID에서 MIME 타입 가져오기
pub fn get_mime_type_from_bindata_id(index: &BinDataIndex, bindata_id: WORD) -> &'static str {
    index
        .get(&bindata_id)
        .and_then(|ext| safe_bindata_extension(ext))
        .map(mime_type_from_safe_extension)
        .unwrap_or("image/jpeg")
}

/// Save image to file and return file path
/// 이미지를 파일로 저장하고 파일 경로 반환
pub fn save_image_to_file(
    index: &BinDataIndex,
    bindata_id: WORD,
    base64_data: &str,
    dir_path: &str,
) -> Result<String, HwpError> {
    let normalized_base64 =
        normalize_base64_image_payload(base64_data).ok_or_else(|| HwpError::InternalError {
            message: "Failed to decode base64: invalid image payload".to_string(),
        })?;

    // base64 디코딩 / Decode base64
    let image_data = STANDARD
        .decode(normalized_base64)
        .map_err(|e| HwpError::InternalError {
            message: format!("Failed to decode base64: {e}"),
        })?;

    // 파일명 생성 / Generate filename
    let extension = detect_extension_from_image_data(&image_data)
        .or_else(|| {
            index
                .get(&bindata_id)
                .and_then(|ext| safe_bindata_extension(ext))
        })
        .unwrap_or("jpg");
    let file_name = format!("BIN{bindata_id:04X}.{extension}");
    let file_path = Path::new(dir_path).join(&file_name);

    // 디렉토리 생성 / Create directory
    fs::create_dir_all(dir_path)
        .map_err(|e| HwpError::Io(format!("Failed to create directory '{dir_path}': {e}")))?;

    // 파일 저장 / Save file
    fs::write(&file_path, &image_data).map_err(|e| {
        HwpError::Io(format!(
            "Failed to write file '{}': {}",
            file_path.display(),
            e
        ))
    })?;

    Ok(file_path.to_string_lossy().to_string())
}

fn safe_bindata_extension(extension: &str) -> Option<&'static str> {
    match extension.trim().to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some("jpg"),
        "png" => Some("png"),
        "gif" => Some("gif"),
        "bmp" => Some("bmp"),
        "webp" => Some("webp"),
        "tif" | "tiff" => Some("tiff"),
        _ => None,
    }
}

fn detect_extension_from_image_data(image_data: &[u8]) -> Option<&'static str> {
    if image_data.starts_with(b"\x89PNG\r\n\x1A\n") {
        Some("png")
    } else if image_data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if image_data.starts_with(b"BM") {
        Some("bmp")
    } else if image_data.starts_with(b"GIF87a") || image_data.starts_with(b"GIF89a") {
        Some("gif")
    } else if image_data.len() >= 12
        && image_data.starts_with(b"RIFF")
        && &image_data[8..12] == b"WEBP"
    {
        Some("webp")
    } else if image_data.starts_with(b"II*\0") || image_data.starts_with(b"MM\0*") {
        Some("tiff")
    } else {
        None
    }
}

fn mime_type_from_safe_extension(extension: &str) -> &'static str {
    match extension {
        "jpg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "tiff" => "image/tiff",
        _ => "image/jpeg",
    }
}

/// Detect MIME type from base64 encoded image data using magic bytes
/// base64 인코딩된 이미지 데이터의 매직 바이트로 MIME 타입 감지
pub fn detect_mime_type_from_base64(base64_data: &str) -> &'static str {
    let header_base64: String = base64_data
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .take(16)
        .map(char::from)
        .collect();

    let Ok(header) = STANDARD.decode(header_base64) else {
        return "application/octet-stream";
    };

    image_mime_type_from_data(&header)
}

fn image_mime_type_from_data(image_data: &[u8]) -> &'static str {
    match detect_extension_from_image_data(image_data) {
        Some(extension) => mime_type_from_safe_extension(extension),
        _ => "application/octet-stream",
    }
}

pub(crate) fn normalize_base64_image_payload(base64_data: &str) -> Option<String> {
    let mut normalized = String::with_capacity(base64_data.len());
    for byte in base64_data.bytes() {
        if byte.is_ascii_whitespace() {
            continue;
        }
        if byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=') {
            normalized.push(char::from(byte));
        } else {
            return None;
        }
    }

    if normalized.is_empty() || normalized.len() % 4 != 0 {
        return None;
    }

    let first_padding = normalized.find('=').unwrap_or(normalized.len());
    let padding_count = normalized.len() - first_padding;
    if padding_count > 2 || !normalized[first_padding..].bytes().all(|byte| byte == b'=') {
        return None;
    }

    Some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::bindata::BinaryDataItem;
    use crate::document::FileHeader;

    fn test_document_with_bindata_items(items: Vec<BinaryDataItem>) -> HwpDocument {
        let mut document = HwpDocument::new(FileHeader {
            signature: "HWP Document File".to_string(),
            version: 0x05000300,
            document_flags: 0,
            license_flags: 0,
            encrypt_version: 0,
            kogl_country: 0,
            reserved: vec![0; 207],
        });
        document.bin_data.items = items;
        document
    }

    #[test]
    fn bindata_item_lookup_finds_items_by_id_and_name() {
        let document = test_document_with_bindata_items(vec![
            BinaryDataItem {
                index: 1,
                data: "first".to_string(),
                name: Some("image1".to_string()),
            },
            BinaryDataItem {
                index: 2,
                data: "second".to_string(),
                name: None,
            },
        ]);

        let lookup = build_bindata_item_lookup(&document);

        assert_eq!(
            lookup.by_id(1).map(|item| item.data.as_str()),
            Some("first")
        );
        assert_eq!(
            lookup.by_id(2).map(|item| item.data.as_str()),
            Some("second")
        );
        assert_eq!(lookup.by_name("image1").map(|item| item.index), Some(1));
        assert!(lookup.by_name("image2").is_none());
    }

    #[test]
    fn bindata_item_lookup_preserves_first_duplicate_to_match_linear_find() {
        let document = test_document_with_bindata_items(vec![
            BinaryDataItem {
                index: 1,
                data: "first".to_string(),
                name: Some("image1".to_string()),
            },
            BinaryDataItem {
                index: 1,
                data: "duplicate".to_string(),
                name: Some("image1".to_string()),
            },
        ]);

        let lookup = build_bindata_item_lookup(&document);

        assert_eq!(
            lookup.by_id(1).map(|item| item.data.as_str()),
            Some("first")
        );
        assert_eq!(
            lookup.by_name("image1").map(|item| item.data.as_str()),
            Some("first")
        );
    }

    #[test]
    fn save_image_to_file_uses_magic_extension_when_index_is_missing() {
        let output_dir = std::env::temp_dir().join(format!(
            "hwp-core-save-image-extension-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&output_dir);
        let index = BinDataIndex::new();

        let saved_path = save_image_to_file(
            &index,
            7,
            "iVBORw0KGgo=",
            output_dir.to_str().expect("test path should be UTF-8"),
        )
        .expect("PNG image should be saved");

        assert!(
            saved_path.ends_with("BIN0007.png"),
            "missing index should fall back to magic-byte extension, got {saved_path}"
        );

        fs::remove_dir_all(&output_dir).expect("test output dir should be removable");
    }

    #[test]
    fn save_image_to_file_accepts_base64_with_ascii_whitespace() {
        let output_dir = std::env::temp_dir().join(format!(
            "hwp-core-save-image-whitespace-base64-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&output_dir);
        let index = BinDataIndex::new();

        let saved_path = save_image_to_file(
            &index,
            7,
            "iVBORw0K\nGgo=",
            output_dir.to_str().expect("test path should be UTF-8"),
        )
        .expect("base64 with ASCII whitespace should be normalized and saved");

        assert!(
            saved_path.ends_with("BIN0007.png"),
            "normalized PNG data should be saved with its magic extension, got {saved_path}"
        );

        fs::remove_dir_all(&output_dir).expect("test output dir should be removable");
    }

    #[test]
    fn save_image_to_file_ignores_unsafe_index_extension() {
        let output_dir = std::env::temp_dir().join(format!(
            "hwp-core-save-image-unsafe-extension-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&output_dir);
        let mut index = BinDataIndex::new();
        index.insert(7, "../evil".to_string());

        let saved_path = save_image_to_file(
            &index,
            7,
            "iVBORw0KGgo=",
            output_dir.to_str().expect("test path should be UTF-8"),
        )
        .expect("unsafe extension should fall back to image magic");

        assert!(
            saved_path.ends_with("BIN0007.png"),
            "unsafe index extensions should not affect output paths, got {saved_path}"
        );
        assert!(Path::new(&saved_path).starts_with(&output_dir));

        fs::remove_dir_all(&output_dir).expect("test output dir should be removable");
    }

    #[test]
    fn save_image_to_file_prefers_magic_extension_over_mismatched_index_extension() {
        let output_dir = std::env::temp_dir().join(format!(
            "hwp-core-save-image-mismatched-extension-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&output_dir);
        let mut index = BinDataIndex::new();
        index.insert(7, "html".to_string());

        let saved_path = save_image_to_file(
            &index,
            7,
            "iVBORw0KGgo=",
            output_dir.to_str().expect("test path should be UTF-8"),
        )
        .expect("PNG image should be saved with its detected extension");

        assert!(
            saved_path.ends_with("BIN0007.png"),
            "recognized image bytes should not be saved with a mismatched declared extension, got {saved_path}"
        );

        fs::remove_dir_all(&output_dir).expect("test output dir should be removable");
    }

    #[test]
    fn save_image_to_file_rejects_active_metadata_extension_when_magic_is_unknown() {
        let output_dir = std::env::temp_dir().join(format!(
            "hwp-core-save-image-active-extension-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&output_dir);
        let mut index = BinDataIndex::new();
        index.insert(7, "html".to_string());

        let saved_path = save_image_to_file(
            &index,
            7,
            "PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==",
            output_dir.to_str().expect("test path should be UTF-8"),
        )
        .expect("unknown bytes should still be saved with a safe fallback extension");

        assert!(
            saved_path.ends_with("BIN0007.jpg"),
            "active metadata extensions should fall back to a safe image extension, got {saved_path}"
        );
        assert!(
            !saved_path.ends_with(".html"),
            "active metadata extensions should not affect output paths, got {saved_path}"
        );

        fs::remove_dir_all(&output_dir).expect("test output dir should be removable");
    }

    #[test]
    fn detect_mime_type_from_base64_does_not_treat_every_riff_as_webp() {
        let riff_wav_header = STANDARD.encode(b"RIFF\x24\x00\x00\x00WAVE");

        assert_eq!(
            detect_mime_type_from_base64(&riff_wav_header),
            "application/octet-stream"
        );
    }
}
