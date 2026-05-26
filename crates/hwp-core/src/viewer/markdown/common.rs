/// 마크다운 변환 공통 유틸리티 함수 / Markdown conversion common utility functions
///
/// 공통 BinData/이미지 함수는 `viewer::shared`에서 재사용합니다.
/// Common BinData/image functions are re-exported from `viewer::shared`.
use std::path::Path;

// 공통 함수 재사용 / Re-use common functions from shared module
use crate::viewer::shared::{detect_mime_type_from_base64, save_image_to_file, BinDataIndex};

/// Format image markdown - either as base64 data URI or file path
/// 이미지 마크다운 포맷 - base64 데이터 URI 또는 파일 경로
pub(crate) fn format_image_markdown(
    bindata_index: &BinDataIndex,
    bindata_id: crate::types::WORD,
    base64_data: &str,
    image_output_dir: Option<&str>,
) -> String {
    format_image_markdown_with_alt(
        bindata_index,
        bindata_id,
        base64_data,
        image_output_dir,
        "image",
    )
}

/// Format image markdown with custom alt text
/// 사용자 지정 alt text로 이미지 마크다운 포맷
pub(crate) fn format_image_markdown_with_alt(
    bindata_index: &BinDataIndex,
    bindata_id: crate::types::WORD,
    base64_data: &str,
    image_output_dir: Option<&str>,
    alt_text: &str,
) -> String {
    match image_output_dir {
        Some(dir_path) => {
            // 이미지를 파일로 저장하고 파일 경로를 마크다운에 포함 / Save image as file and include file path in markdown
            match save_image_to_file(bindata_index, bindata_id, base64_data, dir_path) {
                Ok(file_path) => {
                    // 상대 경로로 변환 (images/ 디렉토리 포함) / Convert to relative path (include images/ directory)
                    let file_path_obj = Path::new(&file_path);
                    let file_name = file_path_obj
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&file_path);
                    // images/ 디렉토리 경로 포함 / Include images/ directory path
                    format!("![{alt_text}](images/{file_name})")
                }
                Err(e) => {
                    eprintln!("Failed to save image: {e}");
                    // 실패 시 base64로 폴백 / Fallback to base64 on failure
                    let mime_type = detect_mime_type_from_base64(base64_data);
                    format!("![{alt_text}](data:{mime_type};base64,{base64_data})")
                }
            }
        }
        None => {
            // base64 데이터 URI로 임베드 / Embed as base64 data URI
            // 매직 바이트로 실제 MIME 타입 감지 (HWPX 등에서 확장자 정보가 없을 때 정확한 MIME 타입 사용)
            // Detect actual MIME type from magic bytes (use accurate MIME type when extension info is missing in HWPX, etc.)
            let mime_type = detect_mime_type_from_base64(base64_data);
            format!("![{alt_text}](data:{mime_type};base64,{base64_data})")
        }
    }
}
