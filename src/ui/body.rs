//! Response bodies for the request inspector: JSON, XML and HTML formatted
//! and highlighted, anything else as plain text

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::response::Detail;

const PUNCT: Color = Color::Indexed(244);
const KEY: Color = Color::Indexed(81);
const STRING: Color = Color::Indexed(150);
const NUMBER: Color = Color::Indexed(215);
const KEYWORD: Color = Color::Indexed(176);
const TAG: Color = Color::Indexed(81);
const ATTR: Color = Color::Indexed(180);
const COMMENT: Color = Color::Indexed(242);
/// Deepest indentation used for markup, so broken HTML can't run off screen
const MAX_DEPTH: usize = 24;
/// Text an element can hold and still be shown on one line with its tags
const INLINE_TEXT: usize = 80;
/// HTML elements that never have a closing tag
const VOID: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Xml,
    Html,
    Text,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Xml => "xml",
            Format::Html => "html",
            Format::Text => "text",
        }
    }

    /// From the Content-Type header, else from how the body starts
    pub fn detect(detail: &Detail, text: &str) -> Format {
        let content_type = detail
            .headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if content_type.contains("json") {
            return Format::Json;
        }
        if content_type.contains("html") {
            return Format::Html;
        }
        if content_type.contains("xml") {
            return Format::Xml;
        }
        let start = text.trim_start();
        let lower: String = start
            .chars()
            .take(15)
            .collect::<String>()
            .to_ascii_lowercase();
        if start.starts_with('{') || start.starts_with('[') {
            Format::Json
        } else if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
            Format::Html
        } else if start.starts_with('<') {
            Format::Xml
        } else {
            Format::Text
        }
    }
}

/// The body as display lines. `raw` shows it as received.
pub fn lines(text: &str, format: Format, complete: bool, raw: bool) -> Vec<Line<'static>> {
    if raw || format == Format::Text {
        return plain(text);
    }
    match format {
        Format::Json => {
            // Pretty-print when it parses; a truncated body can't, but still
            // gets highlighted as it is
            let pretty = complete
                .then(|| serde_json::from_str::<serde_json::Value>(text).ok())
                .flatten()
                .and_then(|v| serde_json::to_string_pretty(&v).ok());
            pretty
                .as_deref()
                .unwrap_or(text)
                .lines()
                .map(json_line)
                .collect()
        }
        Format::Xml => markup(text, false),
        Format::Html => markup(text, true),
        Format::Text => unreachable!(),
    }
}

fn plain(text: &str) -> Vec<Line<'static>> {
    text.lines()
        .map(|l| Line::raw(l.replace('\t', "    ")))
        .collect()
}

fn styled(text: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(color))
}

// ─── JSON ────────────────────────────────────────────────────────────────────

/// One line of JSON with keys, strings, numbers and literals colored
fn json_line(line: &str) -> Line<'static> {
    let chars: Vec<char> = line.chars().collect();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        match c {
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i = (i + 1).min(chars.len());
                let token: String = chars[start..i].iter().collect();
                // A string followed by a colon is a key
                let is_key = chars[i..].iter().find(|c| !c.is_whitespace()) == Some(&':');
                spans.push(styled(token, if is_key { KEY } else { STRING }));
            }
            '-' | '0'..='9' => {
                while i < chars.len() && matches!(chars[i], '0'..='9' | '.' | 'e' | 'E' | '+' | '-')
                {
                    i += 1;
                }
                spans.push(styled(chars[start..i].iter().collect::<String>(), NUMBER));
            }
            c if c.is_ascii_alphabetic() => {
                while i < chars.len() && chars[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                let color = if matches!(word.as_str(), "true" | "false" | "null") {
                    KEYWORD
                } else {
                    Color::Reset
                };
                spans.push(styled(word, color));
            }
            '{' | '}' | '[' | ']' | ',' | ':' => {
                spans.push(styled(c.to_string(), PUNCT));
                i += 1;
            }
            _ => {
                while i < chars.len()
                    && !matches!(
                        chars[i],
                        '"' | '-' | '0'..='9' | '{' | '}' | '[' | ']' | ',' | ':'
                    )
                    && !chars[i].is_ascii_alphabetic()
                {
                    i += 1;
                }
                spans.push(Span::raw(chars[start..i].iter().collect::<String>()));
            }
        }
    }
    Line::from(spans)
}

// ─── XML and HTML ────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum Token<'a> {
    /// `<?xml ...?>`, `<!DOCTYPE ...>`
    Declaration(&'a str),
    Comment(&'a str),
    CData(&'a str),
    Open {
        name: &'a str,
        attrs: &'a str,
        self_closing: bool,
    },
    Close(&'a str),
    Text(&'a str),
}

/// Split markup into tags and text. Forgiving: whatever doesn't parse (a body
/// cut off mid-tag, stray `<`) comes out as text.
fn tokenize(src: &str, html: bool) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut rest = src;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            tokens.push(Token::Text(rest));
            break;
        };
        if lt > 0 {
            tokens.push(Token::Text(&rest[..lt]));
            rest = &rest[lt..];
        }
        let until = |end: &str, from: usize| rest[from..].find(end).map(|i| from + i + end.len());
        let (token, len) = if rest.starts_with("<!--") {
            let end = until("-->", 4).unwrap_or(rest.len());
            (Token::Comment(&rest[..end]), end)
        } else if rest.starts_with("<![CDATA[") {
            let end = until("]]>", 9).unwrap_or(rest.len());
            (Token::CData(&rest[..end]), end)
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            let end = until(">", 2).unwrap_or(rest.len());
            (Token::Declaration(&rest[..end]), end)
        } else if let Some(end) = tag_end(rest) {
            let inner = &rest[1..end - 1];
            if let Some(name) = inner.strip_prefix('/') {
                (Token::Close(name.trim()), end)
            } else {
                let self_closing = inner.ends_with('/');
                let inner = inner.trim_end_matches('/');
                let name_end = inner
                    .find(|c: char| c.is_whitespace())
                    .unwrap_or(inner.len());
                let name = &inner[..name_end];
                if name.is_empty() {
                    (Token::Text("<"), 1)
                } else {
                    (
                        Token::Open {
                            name,
                            attrs: inner[name_end..].trim(),
                            self_closing,
                        },
                        end,
                    )
                }
            }
        } else {
            (Token::Text(rest), rest.len())
        };

        // <script> and <style> hold raw text up to their closing tag
        let raw_text = match &token {
            Token::Open {
                name,
                self_closing: false,
                ..
            } if html && matches!(name.to_ascii_lowercase().as_str(), "script" | "style") => {
                Some(format!("</{}", name.to_ascii_lowercase()))
            }
            _ => None,
        };
        tokens.push(token);
        rest = &rest[len..];
        if let Some(close) = raw_text {
            let end = rest.to_ascii_lowercase().find(&close).unwrap_or(rest.len());
            if end > 0 {
                tokens.push(Token::Text(&rest[..end]));
            }
            rest = &rest[end..];
        }
    }
    tokens
}

/// Index just past the `>` closing the tag that starts `s`, skipping `>`
/// inside quoted attribute values
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices().skip(1) {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '<') => return None,
            (None, '>') => return Some(i + 1),
            _ => {}
        }
    }
    None
}

/// `<name attr="value">` with each part colored
fn tag_spans(name: &str, attrs: &str, self_closing: bool) -> Vec<Span<'static>> {
    let mut spans = vec![styled("<", PUNCT), styled(name, TAG)];
    if !attrs.is_empty() {
        spans.push(Span::raw(" "));
    }
    let mut rest = attrs;
    while !rest.is_empty() {
        let trimmed = rest.trim_start();
        if trimmed.len() < rest.len() {
            spans.push(Span::raw(" "));
            rest = trimmed;
            continue;
        }
        let name_end = rest
            .find(|c: char| c == '=' || c.is_whitespace())
            .unwrap_or(rest.len());
        spans.push(styled(&rest[..name_end], ATTR));
        rest = &rest[name_end..];
        if let Some(after) = rest.strip_prefix('=') {
            spans.push(styled("=", PUNCT));
            let value_end = match after.chars().next() {
                Some(q @ ('"' | '\'')) => after[1..].find(q).map_or(after.len(), |i| i + 2),
                _ => after.find(char::is_whitespace).unwrap_or(after.len()),
            };
            spans.push(styled(&after[..value_end], STRING));
            rest = &after[value_end..];
        }
    }
    spans.push(styled(if self_closing { "/>" } else { ">" }, PUNCT));
    spans
}

fn close_spans(name: &str) -> Vec<Span<'static>> {
    vec![styled("</", PUNCT), styled(name, TAG), styled(">", PUNCT)]
}

/// Collapse runs of whitespace, as a browser would for display
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Re-indent markup by nesting and color it
fn markup(src: &str, html: bool) -> Vec<Line<'static>> {
    let tokens = tokenize(src, html);
    let mut lines = Vec::new();
    let mut depth = 0usize;
    let pad = |depth: usize| Span::raw("  ".repeat(depth.min(MAX_DEPTH)));
    let is_void = |name: &str| html && VOID.contains(&name.to_ascii_lowercase().as_str());
    let raw_text =
        |name: &str| html && matches!(name.to_ascii_lowercase().as_str(), "script" | "style");

    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Open {
                name,
                attrs,
                self_closing,
            } => {
                let mut line = vec![pad(depth)];
                line.extend(tag_spans(name, attrs, *self_closing));
                let opens = !*self_closing && !is_void(name);
                let closes = |t: Option<&Token>| matches!(t, Some(Token::Close(c)) if c.eq_ignore_ascii_case(name));
                // Keep <x></x> and <x>short text</x> on one line
                if opens && closes(tokens.get(i + 1)) {
                    line.extend(close_spans(name));
                    i += 2;
                } else if opens && !raw_text(name) {
                    match (tokens.get(i + 1), closes(tokens.get(i + 2))) {
                        (Some(Token::Text(text)), true)
                            if !text.trim().contains('\n') && squash(text).len() <= INLINE_TEXT =>
                        {
                            line.push(Span::raw(squash(text)));
                            line.extend(close_spans(name));
                            i += 3;
                        }
                        _ => {
                            depth += 1;
                            i += 1;
                        }
                    }
                } else {
                    depth += opens as usize;
                    i += 1;
                }
                lines.push(Line::from(line));
                continue;
            }
            Token::Close(name) => {
                depth = depth.saturating_sub(1);
                let mut line = vec![pad(depth)];
                line.extend(close_spans(name));
                lines.push(Line::from(line));
            }
            Token::Text(text) => {
                let inside_raw = matches!(
                    i.checked_sub(1).and_then(|p| tokens.get(p)),
                    Some(Token::Open { name, .. }) if raw_text(name)
                );
                if inside_raw {
                    // Script and style keep their own layout, re-indented
                    let body: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
                    let common = body
                        .iter()
                        .map(|l| l.len() - l.trim_start().len())
                        .min()
                        .unwrap_or(0);
                    for l in body {
                        lines.push(Line::from(vec![
                            pad(depth),
                            Span::raw(l[common.min(l.len())..].trim_end().replace('\t', "    ")),
                        ]));
                    }
                } else {
                    let text = squash(text);
                    if !text.is_empty() {
                        lines.push(Line::from(vec![pad(depth), Span::raw(text)]));
                    }
                }
            }
            Token::Comment(text) | Token::CData(text) => {
                for l in text.lines() {
                    lines.push(Line::from(vec![
                        pad(depth),
                        Span::styled(
                            l.trim().to_string(),
                            Style::new().fg(COMMENT).add_modifier(Modifier::ITALIC),
                        ),
                    ]));
                }
            }
            Token::Declaration(text) => {
                lines.push(Line::from(vec![pad(depth), styled(squash(text), KEYWORD)]));
            }
        }
        i += 1;
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    fn detail(content_type: &str) -> Detail {
        let mut headers = reqwest::header::HeaderMap::new();
        if !content_type.is_empty() {
            headers.insert("content-type", content_type.parse().unwrap());
        }
        Detail {
            version: reqwest::Version::HTTP_11,
            headers,
            body: Default::default(),
            truncated: false,
            remote_addr: None,
            final_url: String::new(),
        }
    }

    #[test]
    fn detects_formats() {
        assert_eq!(
            Format::detect(&detail("application/problem+json"), ""),
            Format::Json
        );
        assert_eq!(
            Format::detect(&detail("text/html; charset=utf-8"), ""),
            Format::Html
        );
        assert_eq!(
            Format::detect(&detail("application/atom+xml"), ""),
            Format::Xml
        );
        assert_eq!(Format::detect(&detail(""), "  [1]"), Format::Json);
        assert_eq!(
            Format::detect(&detail(""), "<!DOCTYPE html><p>"),
            Format::Html
        );
        assert_eq!(
            Format::detect(&detail(""), "<?xml version=\"1.0\"?>"),
            Format::Xml
        );
        assert_eq!(Format::detect(&detail("text/plain"), "hello"), Format::Text);
    }

    #[test]
    fn json_is_pretty_and_colored() {
        let lines = lines(
            r#"{"a":[1,true,null],"b":"x:y"}"#,
            Format::Json,
            true,
            false,
        );
        assert_eq!(
            text(&lines),
            [
                "{",
                "  \"a\": [",
                "    1,",
                "    true,",
                "    null",
                "  ],",
                "  \"b\": \"x:y\"",
                "}"
            ]
        );
        let key = &lines[1]
            .spans
            .iter()
            .find(|s| s.content == "\"a\"")
            .unwrap();
        assert_eq!(key.style.fg, Some(KEY));
        let value = &lines[6]
            .spans
            .iter()
            .find(|s| s.content == "\"x:y\"")
            .unwrap();
        assert_eq!(
            value.style.fg,
            Some(STRING),
            "a colon inside a string isn't a key"
        );
        assert_eq!(
            lines[3]
                .spans
                .iter()
                .find(|s| s.content == "true")
                .unwrap()
                .style
                .fg,
            Some(KEYWORD)
        );
    }

    #[test]
    fn truncated_json_is_still_highlighted() {
        let lines = lines(r#"{"a": 12, "b": "unfinish"#, Format::Json, false, false);
        assert_eq!(text(&lines), [r#"{"a": 12, "b": "unfinish"#]);
        assert!(lines[0]
            .spans
            .iter()
            .any(|s| s.content == "12" && s.style.fg == Some(NUMBER)));
    }

    #[test]
    fn html_is_indented_with_leaf_elements_inline() {
        let html = r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><title>Directory listing for /</title></head><body><ul><li><a href="a.txt">a.txt</a></li><li></li></ul><br><!-- done --></body></html>"#;
        assert_eq!(
            text(&lines(html, Format::Html, true, false)),
            [
                "<!DOCTYPE html>",
                "<html lang=\"en\">",
                "  <head>",
                "    <meta charset=\"utf-8\">",
                "    <title>Directory listing for /</title>",
                "  </head>",
                "  <body>",
                "    <ul>",
                "      <li>",
                "        <a href=\"a.txt\">a.txt</a>",
                "      </li>",
                "      <li></li>",
                "    </ul>",
                "    <br>",
                "    <!-- done -->",
                "  </body>",
                "</html>",
            ]
        );
    }

    #[test]
    fn script_contents_are_kept_as_written() {
        let html = "<body><script>\n    if (a < b) {\n      go();\n    }\n</script></body>";
        assert_eq!(
            text(&lines(html, Format::Html, true, false)),
            [
                "<body>",
                "  <script>",
                "    if (a < b) {",
                "      go();",
                "    }",
                "  </script>",
                "</body>"
            ]
        );
    }

    #[test]
    fn xml_attributes_and_self_closing_tags() {
        let xml = r#"<?xml version="1.0"?><feed><entry id='1' a="x>y"/><name>pepe</name></feed>"#;
        let out = lines(xml, Format::Xml, true, false);
        assert_eq!(
            text(&out),
            [
                "<?xml version=\"1.0\"?>",
                "<feed>",
                "  <entry id='1' a=\"x>y\"/>",
                "  <name>pepe</name>",
                "</feed>",
            ]
        );
        let entry = &out[2].spans;
        assert!(entry
            .iter()
            .any(|s| s.content == "id" && s.style.fg == Some(ATTR)));
        assert!(entry
            .iter()
            .any(|s| s.content == "\"x>y\"" && s.style.fg == Some(STRING)));
    }

    #[test]
    fn broken_markup_does_not_panic() {
        for src in [
            "<",
            "<a",
            "<a href=\"x",
            "</",
            "text < more",
            "<!--",
            "<![CDATA[x",
            "<script>x",
        ] {
            lines(src, Format::Html, false, false);
            lines(src, Format::Xml, false, false);
        }
    }

    #[test]
    fn raw_shows_the_body_as_received() {
        assert_eq!(
            text(&lines("{\"a\":1}", Format::Json, true, true)),
            ["{\"a\":1}"]
        );
    }
}
