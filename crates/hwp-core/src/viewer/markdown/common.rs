/// 마크다운 변환 공통 유틸리티 함수 / Markdown conversion common utility functions
///
/// 공통 BinData/이미지 함수는 `viewer::shared`에서 재사용합니다.
/// Common BinData/image functions are re-exported from `viewer::shared`.
use std::path::Path;

// 공통 함수 재사용 / Re-use common functions from shared module
use crate::viewer::shared::{
    detect_mime_type_from_base64, normalize_base64_image_payload, save_image_to_file, BinDataIndex,
};

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
    let alt_text = crate::viewer::markdown::security::escape_text(alt_text);
    match image_output_dir {
        Some(dir_path) => {
            // 이미지를 파일로 저장하고 파일 경로를 마크다운에 포함 / Save image as file and include file path in markdown
            match save_image_to_file(bindata_index, bindata_id, base64_data, dir_path) {
                Ok(file_path) => {
                    let image_path = markdown_image_link_path(&file_path);
                    format!("![{alt_text}]({image_path})")
                }
                Err(_e) => {
                    // 실패 시 base64로 폴백 / Fallback to base64 on failure
                    let destination = markdown_image_data_destination(base64_data);
                    format!("![{alt_text}]({destination})")
                }
            }
        }
        None => {
            // base64 데이터 URI로 임베드 / Embed as base64 data URI
            // 매직 바이트로 실제 MIME 타입 감지 (HWPX 등에서 확장자 정보가 없을 때 정확한 MIME 타입 사용)
            // Detect actual MIME type from magic bytes (use accurate MIME type when extension info is missing in HWPX, etc.)
            let destination = markdown_image_data_destination(base64_data);
            format!("![{alt_text}]({destination})")
        }
    }
}

fn markdown_image_data_destination(base64_data: &str) -> String {
    let Some(normalized_base64) = normalize_base64_image_payload(base64_data) else {
        return "#".to_string();
    };
    let mime_type = detect_mime_type_from_base64(&normalized_base64);
    if mime_type == "application/octet-stream" {
        return "#".to_string();
    }

    format!("data:{mime_type};base64,{normalized_base64}")
}

fn markdown_image_link_path(file_path: &str) -> String {
    let file_path_obj = Path::new(file_path);
    let file_name = file_path_obj
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(file_path);
    let parent_name = file_path_obj
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str());

    let destination = parent_name.map_or_else(
        || file_name.to_string(),
        |parent| format!("{parent}/{file_name}"),
    );
    crate::viewer::markdown::security::safe_link_destination(&destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::shared::BinDataIndex;

    #[test]
    fn image_alt_text_escapes_markdown_and_raw_html() {
        let markdown = format_image_markdown_with_alt(
            &BinDataIndex::new(),
            1,
            "not base64",
            None,
            "] (javascript:alert(1)) <img>",
        );

        assert!(!markdown.contains("!] (javascript:alert(1))"));
        assert!(markdown.starts_with(r"![\] (javascript:alert(1)) &lt;img&gt;]("));
    }

    #[test]
    fn embedded_image_markdown_rejects_malformed_base64_link_destination() {
        let markdown = format_image_markdown(
            &BinDataIndex::new(),
            1,
            "iVBORw0KGgo=) [x](javascript:alert(1))",
            None,
        );

        assert_eq!(markdown, "![image](#)");
    }

    #[test]
    fn saved_image_markdown_uses_configured_output_directory_name() {
        let output_dir =
            std::env::temp_dir().join(format!("custom-md-images-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&output_dir);
        let output_dir_str = output_dir.to_str().expect("test path should be UTF-8");

        let markdown = format_image_markdown(
            &BinDataIndex::new(),
            1,
            "iVBORw0KGgo=",
            Some(output_dir_str),
        );

        assert!(
            markdown.contains("](custom-md-images-"),
            "markdown image link should point at the configured output directory, got {markdown}"
        );
        assert!(
            !markdown.contains("](images/"),
            "markdown image link should not use a hard-coded images directory, got {markdown}"
        );

        std::fs::remove_dir_all(&output_dir).expect("test output dir should be removable");
    }

    #[test]
    fn saved_image_markdown_escapes_output_directory_link_destination() {
        let output_dir =
            std::env::temp_dir().join(format!("custom-md-images-)-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&output_dir);
        let output_dir_str = output_dir.to_str().expect("test path should be UTF-8");

        let markdown = format_image_markdown(
            &BinDataIndex::new(),
            1,
            "iVBORw0KGgo=",
            Some(output_dir_str),
        );

        assert!(
            markdown.contains(r"custom-md-images-\)-"),
            "markdown image links should escape ')' in generated destinations, got {markdown}"
        );

        std::fs::remove_dir_all(&output_dir).expect("test output dir should be removable");
    }
}
