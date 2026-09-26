//! Credential-safe diagnostic text for persistence and events.

/// Longest error text we keep in the database.
pub(crate) const DETAIL_LIMIT: usize = 400;

const REDACTED: &str = "<redacted>";

/// Credential prefixes whose value follows the prefix directly.
const SECRET_PREFIXES: [&str; 5] = ["ak-", "as-", "sk-", "ghp_", "Bearer "];

/// `WORD = value`, `WORD: value`, `"WORD": "value"` style assignments.
const SECRET_ASSIGNMENTS: [&str; 2] = ["MODAL_TOKEN_SECRET", "token"];

/// Characters that can appear inside a credential value.
fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '-' | '_' | '.' | '+' | '/' | '=' | ':' | '@' | '~' | '%' | '&' | '#' | '!' | '*' | '$'
        )
}

/// Length of the credential value starting at `tail`, including any surrounding
/// quotes. Returns 0 when nothing value-like follows, so a marker is never added
/// without a value.
fn value_span(tail: &str) -> usize {
    let mut start = 0;
    for (index, c) in tail.char_indices() {
        if c.is_whitespace() || matches!(c, '=' | ':' | ',' | ';') {
            start = index + c.len_utf8();
        } else {
            break;
        }
    }
    let rest = &tail[start..];
    let Some(first) = rest.chars().next() else {
        return 0;
    };
    if matches!(first, '"' | '\'' | '`') {
        // A quoted value runs to its closing quote, honouring backslash escapes,
        // and stops at the line end when the quote is never closed.
        let after = &rest[first.len_utf8()..];
        let mut end = after.len();
        let mut escaped = false;
        for (offset, c) in after.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if c == '\\' {
                escaped = true;
                continue;
            }
            if c == first {
                end = offset + c.len_utf8();
                break;
            }
            if c == '\n' {
                end = offset;
                break;
            }
        }
        return start + first.len_utf8() + end;
    }
    let run = rest.find(|c: char| !is_token_char(c)).unwrap_or(rest.len());
    start + run
}

/// Replaces the credential after every `prefix` occurrence. With
/// `require_separator` the word only counts as a marker when whitespace and a
/// `=` or `:` separator follow it, which keeps prose like "token expired" intact.
fn redact_after(text: &str, prefix: &str, require_separator: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(prefix) {
        let (head, tail) = rest.split_at(index + prefix.len());
        out.push_str(head);
        let mut value_start = 0;
        if require_separator {
            let mut separated = false;
            for (offset, c) in tail.char_indices() {
                if matches!(c, '=' | ':') {
                    separated = true;
                    value_start = offset + c.len_utf8();
                    break;
                }
                // JSON writes the separator after the quoted key: "token":"value"
                if c.is_whitespace() || matches!(c, '"' | '\'' | '`') {
                    value_start = offset + c.len_utf8();
                    continue;
                }
                break;
            }
            if !separated {
                rest = tail;
                continue;
            }
        }
        let span = value_span(&tail[value_start..]);
        if span == 0 {
            // Nothing value-like follows, so leave the text exactly as it was.
            rest = tail;
            continue;
        }
        if require_separator {
            out.push_str(&tail[..value_start]);
        }
        out.push_str(REDACTED);
        rest = &tail[value_start + span..];
    }
    out.push_str(rest);
    out
}

/// One marker per secret: nested markers and repeated redactions collapse.
fn collapse_redactions(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(REDACTED) {
        out.push_str(&rest[..index]);
        out.push_str(REDACTED);
        let mut tail = &rest[index + REDACTED.len()..];
        loop {
            let trimmed = tail.trim_start_matches(char::is_whitespace);
            if trimmed.starts_with(REDACTED) {
                tail = &trimmed[REDACTED.len()..];
            } else {
                break;
            }
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Makes CLI and worker output safe to persist and to show: credentials are
/// redacted, whitespace collapsed, and the length capped.
pub(crate) fn sanitize_detail(raw: &str, max: usize) -> String {
    let mut text = raw.trim().to_string();
    for prefix in SECRET_PREFIXES {
        if text.contains(prefix) {
            text = redact_after(&text, prefix, false);
        }
    }
    for word in SECRET_ASSIGNMENTS {
        if text.contains(word) {
            text = redact_after(&text, word, true);
        }
    }
    text = collapse_redactions(&text);
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut clipped: String = flat.chars().take(max).collect();
    clipped.push('…');
    clipped
}
