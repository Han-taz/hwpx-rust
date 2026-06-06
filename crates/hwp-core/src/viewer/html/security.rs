pub(crate) fn escape_html_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) fn escape_html_attr(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) fn safe_href_value(url: &str) -> &str {
    let trimmed = url.trim();
    if is_safe_href(trimmed) {
        trimmed
    } else {
        "#"
    }
}

fn is_safe_href(url: &str) -> bool {
    if url.is_empty() || url.chars().any(char::is_control) {
        return false;
    }

    let scheme_end = url
        .char_indices()
        .find_map(|(idx, ch)| match ch {
            ':' => Some(Some(idx)),
            '/' | '?' | '#' => Some(None),
            _ => None,
        })
        .flatten();

    let Some(scheme_end) = scheme_end else {
        return true;
    };

    let scheme = &url[..scheme_end];
    if scheme.is_empty()
        || !scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    {
        return false;
    }

    matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "mailto" | "tel"
    )
}

pub(crate) fn escape_css_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\22 "),
            '\'' => escaped.push_str("\\27 "),
            '\\' => escaped.push_str("\\5c "),
            '(' => escaped.push_str("\\28 "),
            ')' => escaped.push_str("\\29 "),
            '\n' => escaped.push_str("\\a "),
            '\r' => escaped.push_str("\\d "),
            '\t' => escaped.push_str("\\9 "),
            ch if ch.is_control() => {
                use std::fmt::Write;
                let _ = write!(escaped, "\\{:x} ", ch as u32);
            }
            ch => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn href_safety_allows_common_safe_targets() {
        assert_eq!(
            safe_href_value("https://example.com/a?b=c"),
            "https://example.com/a?b=c"
        );
        assert_eq!(
            safe_href_value("mailto:test@example.com"),
            "mailto:test@example.com"
        );
        assert_eq!(safe_href_value("#section-1"), "#section-1");
        assert_eq!(safe_href_value("../relative/path"), "../relative/path");
    }

    #[test]
    fn href_safety_rejects_active_content_schemes() {
        assert_eq!(safe_href_value("javascript:alert(1)"), "#");
        assert_eq!(safe_href_value("JaVaScRiPt:alert(1)"), "#");
        assert_eq!(safe_href_value("data:text/html,<svg onload=alert(1)>"), "#");
        assert_eq!(safe_href_value("java\nscript:alert(1)"), "#");
    }
}
