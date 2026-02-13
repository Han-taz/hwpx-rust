/// Field control conversion to Markdown
/// 필드 컨트롤을 마크다운으로 변환
///
/// Handles hyperlinks and other field types
/// 하이퍼링크 및 기타 필드 타입 처리
use crate::document::{CtrlHeader, CtrlHeaderData, CtrlId};

/// Convert field control to markdown
/// 필드 컨트롤을 마크다운으로 변환
///
/// # Arguments
/// * `header` - Control header containing field data
/// * `text` - Optional text content for the hyperlink
///
/// # Returns
/// Markdown string for the field
pub fn convert_field_ctrl_to_markdown(header: &CtrlHeader, text: Option<&str>) -> String {
    if let CtrlHeaderData::Field {
        field_type,
        command,
        ..
    } = &header.data
    {
        match field_type.as_str() {
            // Hyperlink field - %hlk
            hlk if hlk == CtrlId::FIELD_HYPERLINK.trim_start_matches('%')
                || field_type == "%hlk" =>
            {
                convert_hyperlink_to_markdown(command, text)
            }
            // Date field
            dte if dte == "dte" || field_type == "%dte" => {
                // TODO: Parse date format from command and return formatted date
                "[날짜]".to_string()
            }
            // Page number field
            pgn if pgn == "pgn" || field_type == "%pgn" => "[페이지]".to_string(),
            // Other fields - return command or placeholder
            _ => {
                if !command.is_empty() {
                    command.clone()
                } else {
                    String::new()
                }
            }
        }
    } else {
        String::new()
    }
}

/// Extract URL from HWP field command
/// HWP 필드 명령에서 URL 추출
///
/// HWP format: "url;extra;params;" or just "url"
/// The actual URL is the part before the first semicolon
fn extract_url_from_command(command: &str) -> &str {
    // Clean up the command (remove trailing nulls/whitespace)
    let clean_cmd = command.trim().trim_end_matches('\0');

    // Split by semicolon and take the first part as URL
    clean_cmd.split(';').next().unwrap_or(clean_cmd)
}

/// Convert hyperlink to markdown format
/// 하이퍼링크를 마크다운 형식으로 변환
///
/// # Arguments
/// * `command` - The command from the field (format: "url;extra;params;")
/// * `text` - Optional display text
///
/// # Returns
/// Markdown hyperlink: [text](url) or <url>
fn convert_hyperlink_to_markdown(command: &str, text: Option<&str>) -> String {
    let url = extract_url_from_command(command);

    if url.is_empty() {
        return String::new();
    }

    match text {
        Some(display_text) if !display_text.is_empty() => {
            format!("[{}]({})", display_text.trim(), url)
        }
        _ => {
            // If no text, use the URL as both text and link
            format!("<{}>", url)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hyperlink_with_text() {
        let result = convert_hyperlink_to_markdown("https://example.com", Some("Example Site"));
        assert_eq!(result, "[Example Site](https://example.com)");
    }

    #[test]
    fn test_hyperlink_without_text() {
        let result = convert_hyperlink_to_markdown("https://example.com", None);
        assert_eq!(result, "<https://example.com>");
    }

    #[test]
    fn test_hyperlink_empty_url() {
        let result = convert_hyperlink_to_markdown("", Some("Text"));
        assert_eq!(result, "");
    }
}
