/// 공통 유틸리티 함수 / Common utility functions
///
/// 마크다운 변환에 사용되는 공통 함수들을 제공합니다.
/// Provides common functions used in markdown conversion.
use crate::document::{BinDataRecord, HwpDocument};
use crate::error::HwpError;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::fs;
use std::path::Path;

/// Detect MIME type from base64 encoded image data using magic bytes
/// base64 인코딩된 이미지 데이터의 매직 바이트로 MIME 타입 감지
///
/// Magic bytes (base64 encoded):
/// - PNG: starts with "iVBORw" (0x89 0x50 0x4E 0x47)
/// - JPEG: starts with "/9j/" (0xFF 0xD8 0xFF)
/// - BMP: starts with "Qk" (0x42 0x4D = "BM")
pub(crate) fn detect_mime_type_from_base64(base64_data: &str) -> &'static str {
    if base64_data.starts_with("iVBORw") {
        "image/png"
    } else if base64_data.starts_with("/9j/") {
        "image/jpeg"
    } else if base64_data.starts_with("Qk") {
        "image/bmp"
    } else {
        "application/octet-stream"
    }
}

/// Get MIME type from BinData ID using bin_data_records
/// bin_data_records를 사용하여 BinData ID에서 MIME 타입 가져오기
pub(crate) fn get_mime_type_from_bindata_id(
    document: &HwpDocument,
    bindata_id: crate::types::WORD,
) -> String {
    // bin_data_records에서 EMBEDDING 타입의 extension 찾기 / Find extension from EMBEDDING type in bin_data_records
    for record in &document.doc_info.bin_data {
        if let BinDataRecord::Embedding { embedding, .. } = record {
            if embedding.binary_data_id == bindata_id {
                return match embedding.extension.to_lowercase().as_str() {
                    "jpg" | "jpeg" => "image/jpeg",
                    "png" => "image/png",
                    "gif" => "image/gif",
                    "bmp" => "image/bmp",
                    _ => "image/jpeg", // 기본값 / default
                }
                .to_string();
            }
        }
    }
    // 기본값 / default
    "image/jpeg".to_string()
}

/// Get file extension from BinData ID using bin_data_records
/// bin_data_records를 사용하여 BinData ID에서 파일 확장자 가져오기
pub(crate) fn get_extension_from_bindata_id(
    document: &HwpDocument,
    bindata_id: crate::types::WORD,
) -> String {
    // bin_data_records에서 EMBEDDING 타입의 extension 찾기 / Find extension from EMBEDDING type in bin_data_records
    for record in &document.doc_info.bin_data {
        if let BinDataRecord::Embedding { embedding, .. } = record {
            if embedding.binary_data_id == bindata_id {
                return embedding.extension.clone();
            }
        }
    }
    // 기본값 / default
    "jpg".to_string()
}

/// Format image markdown - either as base64 data URI or file path
/// 이미지 마크다운 포맷 - base64 데이터 URI 또는 파일 경로
pub(crate) fn format_image_markdown(
    document: &HwpDocument,
    bindata_id: crate::types::WORD,
    base64_data: &str,
    image_output_dir: Option<&str>,
) -> String {
    match image_output_dir {
        Some(dir_path) => {
            // 이미지를 파일로 저장하고 파일 경로를 마크다운에 포함 / Save image as file and include file path in markdown
            match save_image_to_file(document, bindata_id, base64_data, dir_path) {
                Ok(file_path) => {
                    // 상대 경로로 변환 (images/ 디렉토리 포함) / Convert to relative path (include images/ directory)
                    let file_path_obj = Path::new(&file_path);
                    let file_name = file_path_obj
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&file_path);
                    // images/ 디렉토리 경로 포함 / Include images/ directory path
                    format!("![이미지](images/{file_name})")
                }
                Err(e) => {
                    eprintln!("Failed to save image: {e}");
                    // 실패 시 base64로 폴백 / Fallback to base64 on failure
                    let mime_type = detect_mime_type_from_base64(base64_data);
                    format!("![이미지](data:{mime_type};base64,{base64_data})")
                }
            }
        }
        None => {
            // base64 데이터 URI로 임베드 / Embed as base64 data URI
            // 매직 바이트로 실제 MIME 타입 감지 (HWPX 등에서 확장자 정보가 없을 때 정확한 MIME 타입 사용)
            // Detect actual MIME type from magic bytes (use accurate MIME type when extension info is missing in HWPX, etc.)
            let mime_type = detect_mime_type_from_base64(base64_data);
            format!("![이미지](data:{mime_type};base64,{base64_data})")
        }
    }
}

/// Save image to file from base64 data
/// base64 데이터에서 이미지를 파일로 저장
fn save_image_to_file(
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
