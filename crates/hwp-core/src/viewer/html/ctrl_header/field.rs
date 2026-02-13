/// Field control conversion to HTML
/// 필드 컨트롤을 HTML로 변환
///
/// Handles hyperlinks and other field types
/// 하이퍼링크 및 기타 필드 타입 처리
use crate::document::{CtrlHeader, CtrlHeaderData, CtrlId};

/// Convert field control to HTML
/// 필드 컨트롤을 HTML로 변환
///
/// # Arguments
/// * `header` - Control header containing field data
/// * `text` - Optional text content for the hyperlink
///
/// # Returns
/// HTML string for the field
#[allow(dead_code)]
pub fn convert_field_ctrl_to_html(header: &CtrlHeader, text: Option<&str>) -> String {
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
                convert_hyperlink_to_html(command, text)
            }
            // Date field
            dte if dte == "dte" || field_type == "%dte" => {
                // TODO: Parse date format from command and return formatted date
                "<span class=\"field-date\">[날짜]</span>".to_string()
            }
            // Page number field
            pgn if pgn == "pgn" || field_type == "%pgn" => {
                "<span class=\"field-page\">[페이지]</span>".to_string()
            }
            // Other fields - return command or placeholder
            _ => {
                if !command.is_empty() {
                    // Escape HTML special characters
                    html_escape(command)
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
#[allow(dead_code)]
fn extract_url_from_command(command: &str) -> &str {
    // Clean up the command (remove trailing nulls/whitespace)
    let clean_cmd = command.trim().trim_end_matches('\0');

    // Split by semicolon and take the first part as URL
    clean_cmd.split(';').next().unwrap_or(clean_cmd)
}

/// Convert hyperlink to HTML format
/// 하이퍼링크를 HTML 형식으로 변환
///
/// # Arguments
/// * `command` - The command from the field (format: "url;extra;params;")
/// * `text` - Optional display text
///
/// # Returns
/// HTML hyperlink: <a href="url">text</a> or <a href="url">url</a>
#[allow(dead_code)]
fn convert_hyperlink_to_html(command: &str, text: Option<&str>) -> String {
    let url = extract_url_from_command(command);

    if url.is_empty() {
        return String::new();
    }

    // Escape URL for HTML attribute
    let escaped_url = html_escape_attr(url);

    match text {
        Some(display_text) if !display_text.is_empty() => {
            let escaped_text = html_escape(display_text.trim());
            format!(r#"<a href="{escaped_url}" class="hwp-link">{escaped_text}</a>"#)
        }
        _ => {
            // If no text, use the URL as both text and link
            let escaped_text = html_escape(url);
            format!(r#"<a href="{escaped_url}" class="hwp-link">{escaped_text}</a>"#)
        }
    }
}

/// Escape HTML special characters in text content
/// 텍스트 콘텐츠의 HTML 특수 문자 이스케이프
#[allow(dead_code)]
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Escape characters for HTML attribute values
/// HTML 속성 값을 위한 문자 이스케이프
#[allow(dead_code)]
fn html_escape_attr(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hyperlink_with_text() {
        let result = convert_hyperlink_to_html("https://example.com", Some("Example Site"));
        assert_eq!(
            result,
            r#"<a href="https://example.com" class="hwp-link">Example Site</a>"#
        );
    }

    #[test]
    fn test_hyperlink_without_text() {
        let result = convert_hyperlink_to_html("https://example.com", None);
        assert_eq!(
            result,
            r#"<a href="https://example.com" class="hwp-link">https://example.com</a>"#
        );
    }

    #[test]
    fn test_hyperlink_empty_url() {
        let result = convert_hyperlink_to_html("", Some("Text"));
        assert_eq!(result, "");
    }

    #[test]
    fn test_hyperlink_with_special_chars() {
        let result = convert_hyperlink_to_html(
            "https://example.com/search?q=test&page=1",
            Some("Search <Results>"),
        );
        assert_eq!(
            result,
            r#"<a href="https://example.com/search?q=test&amp;page=1" class="hwp-link">Search &lt;Results&gt;</a>"#
        );
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape(r#"say "hello""#), "say &quot;hello&quot;");
    }
}
