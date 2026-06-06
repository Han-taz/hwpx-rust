pub(crate) fn escape_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '[' => escaped.push_str("\\["),
            ']' => escaped.push_str("\\]"),
            '\\' => escaped.push_str("\\\\"),
            ch => escaped.push(ch),
        }
    }
    escaped
}

pub(crate) fn format_link(label: &str, url: &str) -> String {
    let escaped_label = escape_text(label);
    format_link_escaped_label(&escaped_label, url)
}

pub(crate) fn format_link_escaped_label(label: &str, url: &str) -> String {
    let destination = safe_link_destination(url);
    format!("[{label}]({destination})")
}

pub(crate) fn format_autolink_or_link(url: &str) -> String {
    let trimmed = url.trim();
    let destination = safe_link_destination(trimmed);
    if destination == trimmed && can_use_autolink(trimmed) {
        format!("<{trimmed}>")
    } else {
        let label = escape_text(trimmed);
        format!("[{label}]({destination})")
    }
}

fn can_use_autolink(url: &str) -> bool {
    !url.is_empty()
        && !url
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '<' | '>' | '\\'))
}

pub(crate) fn safe_link_destination(url: &str) -> String {
    let trimmed = url.trim();
    if !is_safe_link_url(trimmed) {
        return "#".to_string();
    }

    let mut escaped = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            ')' => escaped.push_str("\\)"),
            '\n' | '\r' | '\t' => escaped.push(' '),
            ch => escaped.push(ch),
        }
    }
    escaped
}

fn is_safe_link_url(url: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_destinations_allow_common_safe_targets() {
        assert_eq!(
            safe_link_destination("https://example.com"),
            "https://example.com"
        );
        assert_eq!(safe_link_destination("#section"), "#section");
        assert_eq!(safe_link_destination("../relative"), "../relative");
    }

    #[test]
    fn link_destinations_reject_active_content_schemes() {
        assert_eq!(safe_link_destination("javascript:alert(1)"), "#");
        assert_eq!(safe_link_destination("data:text/html,<svg>"), "#");
        assert_eq!(safe_link_destination("java\nscript:alert(1)"), "#");
    }
}
