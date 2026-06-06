/// HTML 뷰어 공통 유틸리티 함수 / HTML viewer common utility functions
///
/// 공통 BinData 함수는 `viewer::shared`에서 재사용합니다.
/// Common BinData functions are re-exported from `viewer::shared`.
use crate::WORD;
use std::path::Path;

// 공통 함수 재사용 / Re-use common functions from shared module
use crate::viewer::shared::{
    detect_mime_type_from_base64, get_mime_type_from_bindata_id, normalize_base64_image_payload,
    save_image_to_file, BinDataIndex, BinDataItemLookup,
};

/// Get image URL using a pre-built BinData item lookup.
pub(crate) fn get_image_url_with_lookup(
    bindata_index: &BinDataIndex,
    bindata_lookup: &BinDataItemLookup<'_>,
    bindata_id: WORD,
    image_output_dir: Option<&str>,
    html_output_dir: Option<&str>,
) -> String {
    // BinData에서 이미지 데이터 찾기 / Find image data from BinData
    let base64_data = bindata_lookup
        .by_id(bindata_id)
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
                        html_image_link_path(&file_path)
                    }
                }
                Err(_) => {
                    // 실패 시 base64로 폴백 / Fallback to base64 on failure
                    image_data_uri(bindata_index, bindata_id, base64_data)
                }
            }
        }
        None => {
            // base64 데이터 URI로 임베드 / Embed as base64 data URI
            image_data_uri(bindata_index, bindata_id, base64_data)
        }
    }
}

fn image_data_uri(bindata_index: &BinDataIndex, bindata_id: WORD, base64_data: &str) -> String {
    let Some(normalized_base64) = normalize_base64_image_payload(base64_data) else {
        return "#".to_string();
    };
    let mime_type = image_data_uri_mime_type(bindata_index, bindata_id, &normalized_base64);
    format!("data:{mime_type};base64,{normalized_base64}")
}

fn image_data_uri_mime_type(
    bindata_index: &BinDataIndex,
    bindata_id: WORD,
    base64_data: &str,
) -> &'static str {
    let detected = detect_mime_type_from_base64(base64_data);
    if detected == "application/octet-stream" {
        get_mime_type_from_bindata_id(bindata_index, bindata_id)
    } else {
        detected
    }
}

fn html_image_link_path(file_path: &str) -> String {
    let file_path_obj = Path::new(file_path);
    let file_name = file_path_obj
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(file_path);
    let parent_name = file_path_obj
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str());

    parent_name.map_or_else(
        || file_name.to_string(),
        |parent| format!("{parent}/{file_name}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::bindata::BinaryDataItem;
    use crate::document::{FileHeader, HwpDocument};

    fn test_document_with_bindata(index: WORD, data: &str) -> HwpDocument {
        let mut document = HwpDocument::new(FileHeader {
            signature: "HWP Document File".to_string(),
            version: 0x05000300,
            document_flags: 0,
            license_flags: 0,
            encrypt_version: 0,
            kogl_country: 0,
            reserved: vec![0; 207],
        });
        document.bin_data.items.push(BinaryDataItem {
            index,
            data: data.to_string(),
            name: Some("image1".to_string()),
        });
        document
    }

    fn test_document_with_png() -> HwpDocument {
        test_document_with_bindata(1, "iVBORw0KGgo=")
    }

    fn test_image_url(
        document: &HwpDocument,
        bindata_index: &BinDataIndex,
        bindata_id: WORD,
        image_output_dir: Option<&str>,
        html_output_dir: Option<&str>,
    ) -> String {
        let bindata_lookup = crate::viewer::shared::build_bindata_item_lookup(document);
        get_image_url_with_lookup(
            bindata_index,
            &bindata_lookup,
            bindata_id,
            image_output_dir,
            html_output_dir,
        )
    }

    #[test]
    fn saved_html_image_url_uses_configured_output_directory_name_without_html_dir() {
        let output_dir =
            std::env::temp_dir().join(format!("custom-html-images-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&output_dir);
        let output_dir_str = output_dir.to_str().expect("test path should be UTF-8");
        let document = test_document_with_png();

        let image_url = test_image_url(
            &document,
            &BinDataIndex::new(),
            1,
            Some(output_dir_str),
            None,
        );

        assert!(
            image_url.starts_with("custom-html-images-"),
            "HTML image URL should point at the configured output directory, got {image_url}"
        );
        assert!(
            image_url.ends_with("/BIN0001.png"),
            "HTML image URL should include the saved image file name, got {image_url}"
        );

        std::fs::remove_dir_all(&output_dir).expect("test output dir should be removable");
    }

    #[test]
    fn embedded_html_image_data_uri_uses_magic_mime_when_index_is_missing() {
        let document = test_document_with_png();

        let image_url = test_image_url(&document, &BinDataIndex::new(), 1, None, None);

        assert_eq!(image_url, "data:image/png;base64,iVBORw0KGgo=");
    }

    #[test]
    fn embedded_html_image_data_uri_uses_prebuilt_bindata_lookup() {
        let document = test_document_with_png();
        let bindata_lookup = crate::viewer::shared::build_bindata_item_lookup(&document);

        let image_url =
            get_image_url_with_lookup(&BinDataIndex::new(), &bindata_lookup, 1, None, None);

        assert_eq!(image_url, "data:image/png;base64,iVBORw0KGgo=");
    }

    #[test]
    fn embedded_html_image_data_uri_rejects_malformed_base64_payload() {
        let document = test_document_with_bindata(
            2,
            "iVBORw0KGgo=');background-image:url(javascript:alert(1));",
        );

        let image_url = test_image_url(&document, &BinDataIndex::new(), 2, None, None);

        assert_eq!(image_url, "#");
    }

    #[test]
    fn embedded_html_image_data_uri_falls_back_to_webp_metadata_when_magic_is_unknown() {
        let document = test_document_with_bindata(2, "Zm9vYmFy");
        let mut bindata_index = BinDataIndex::new();
        bindata_index.insert(2, "webp".to_string());

        let image_url = test_image_url(&document, &bindata_index, 2, None, None);

        assert_eq!(image_url, "data:image/webp;base64,Zm9vYmFy");
    }
}
