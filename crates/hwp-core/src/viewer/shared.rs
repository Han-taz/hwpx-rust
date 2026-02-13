/// 뷰어 간 공통 유틸리티 함수 / Shared utility functions across viewers
///
/// HTML 뷰어와 Markdown 뷰어에서 공통으로 사용하는 함수들입니다.
/// Functions shared between HTML and Markdown viewers.
use crate::document::{BinDataRecord, HwpDocument};
use crate::error::HwpError;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::fs;
use std::path::Path;

/// Get file extension from BinData ID
/// BinData ID에서 파일 확장자 가져오기
pub fn get_extension_from_bindata_id(
    document: &HwpDocument,
    bindata_id: crate::types::WORD,
) -> String {
    for record in &document.doc_info.bin_data {
        if let BinDataRecord::Embedding { embedding, .. } = record {
            if embedding.binary_data_id == bindata_id {
                return embedding.extension.clone();
            }
        }
    }
    "jpg".to_string()
}

/// Get MIME type from BinData ID
/// BinData ID에서 MIME 타입 가져오기
pub fn get_mime_type_from_bindata_id(
    document: &HwpDocument,
    bindata_id: crate::types::WORD,
) -> String {
    for record in &document.doc_info.bin_data {
        if let BinDataRecord::Embedding { embedding, .. } = record {
            if embedding.binary_data_id == bindata_id {
                return match embedding.extension.to_lowercase().as_str() {
                    "jpg" | "jpeg" => "image/jpeg",
                    "png" => "image/png",
                    "gif" => "image/gif",
                    "bmp" => "image/bmp",
                    _ => "image/jpeg",
                }
                .to_string();
            }
        }
    }
    "image/jpeg".to_string()
}

/// Save image to file and return file path
/// 이미지를 파일로 저장하고 파일 경로 반환
pub fn save_image_to_file(
    document: &HwpDocument,
    bindata_id: crate::types::WORD,
    base64_data: &str,
    dir_path: &str,
) -> Result<String, HwpError> {
    // base64 디코딩 / Decode base64
    let image_data = STANDARD
        .decode(base64_data)
        .map_err(|e| HwpError::InternalError {
            message: format!("Failed to decode base64: {e}"),
        })?;

    // 파일명 생성 / Generate filename
    let extension = get_extension_from_bindata_id(document, bindata_id);
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

/// Detect MIME type from base64 encoded image data using magic bytes
/// base64 인코딩된 이미지 데이터의 매직 바이트로 MIME 타입 감지
pub fn detect_mime_type_from_base64(base64_data: &str) -> &'static str {
    if base64_data.starts_with("iVBORw") {
        "image/png"
    } else if base64_data.starts_with("/9j/") {
        "image/jpeg"
    } else if base64_data.starts_with("Qk") {
        "image/bmp"
    } else if base64_data.starts_with("R0lGOD") {
        // GIF87a/GIF89a: 0x47 0x49 0x46 0x38 → base64 "R0lGOD"
        "image/gif"
    } else if base64_data.starts_with("UklGR") {
        // RIFF header (WebP): 0x52 0x49 0x46 0x46 → base64 "UklGR"
        "image/webp"
    } else if base64_data.starts_with("SUkq") || base64_data.starts_with("TU0A") {
        // TIFF little-endian (II*): 0x49 0x49 0x2A 0x00 → base64 "SUkq"
        // TIFF big-endian (MM): 0x4D 0x4D 0x00 → base64 "TU0A"
        "image/tiff"
    } else {
        "application/octet-stream"
    }
}
