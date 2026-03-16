/// HTML 뷰어 공통 유틸리티 함수 / HTML viewer common utility functions
///
/// 공통 BinData 함수는 `viewer::shared`에서 재사용합니다.
/// Common BinData functions are re-exported from `viewer::shared`.
use crate::document::HwpDocument;
use crate::WORD;
use std::path::Path;

// 공통 함수 재사용 / Re-use common functions from shared module
use crate::viewer::shared::{get_mime_type_from_bindata_id, save_image_to_file, BinDataIndex};

/// Get image URL (file path or base64 data URI)
/// 이미지 URL 가져오기 (파일 경로 또는 base64 데이터 URI)
pub fn get_image_url(
    document: &HwpDocument,
    bindata_index: &BinDataIndex,
    bindata_id: WORD,
    image_output_dir: Option<&str>,
    html_output_dir: Option<&str>,
) -> String {
    // BinData에서 이미지 데이터 찾기 / Find image data from BinData
    let base64_data = document
        .bin_data
        .items
        .iter()
        .find(|item| item.index == bindata_id)
        .map(|item| item.data.as_str())
        .unwrap_or("");

    if base64_data.is_empty() {
        return String::new();
    }

    match image_output_dir {
        Some(dir_path) => {
            // 이미지를 파일로 저장 / Save image as file
            match save_image_to_file(bindata_index, bindata_id, base64_data, dir_path) {
                Ok(file_path) => {
                    // HTML 출력 디렉토리가 있으면 상대 경로 계산 / Calculate relative path if HTML output directory is provided
                    if let Some(html_dir) = html_output_dir {
                        let image_path = Path::new(&file_path);
                        let html_path = Path::new(html_dir);

                        // 상대 경로 계산 / Calculate relative path
                        match pathdiff::diff_paths(image_path, html_path) {
                            Some(relative_path) => {
                                // 경로 구분자를 슬래시로 통일 / Normalize path separators to forward slashes
                                relative_path.to_string_lossy().replace('\\', "/")
                            }
                            None => {
                                // 상대 경로 계산 실패 시 파일명만 반환 / Return filename only if relative path calculation fails
                                image_path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| file_path)
                            }
                        }
                    } else {
                        // HTML 출력 디렉토리가 없으면 파일명만 반환 / Return filename only if HTML output directory is not provided
                        let file_path_obj = Path::new(&file_path);
                        file_path_obj
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&file_path)
                            .to_string()
                    }
                }
                Err(_) => {
                    // 실패 시 base64로 폴백 / Fallback to base64 on failure
                    let mime_type = get_mime_type_from_bindata_id(bindata_index, bindata_id);
                    format!("data:{mime_type};base64,{base64_data}")
                }
            }
        }
        None => {
            // base64 데이터 URI로 임베드 / Embed as base64 data URI
            let mime_type = get_mime_type_from_bindata_id(bindata_index, bindata_id);
            format!("data:{mime_type};base64,{base64_data}")
        }
    }
}
