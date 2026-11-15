use std::collections::HashSet;
use std::sync::OnceLock;

use journal_types::{Entry, EntryFrontmatter};
use pulldown_cmark::{html, Event, Options, Parser, Tag, TagEnd};
use regex::Regex;

use crate::error::{Result, VaultError};

fn wikilink_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[\[([^\[\]\r\n]+)\]\]").expect("valid wikilink regex"))
}

/// Convert an arbitrary title or name into a filesystem-safe lowercase slug.
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len().min(64));
    let mut prev_hyphen = true;

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen && !out.is_empty() {
            out.push('-');
            prev_hyphen = true;
        }
        if out.len() >= 64 {
            break;
        }
    }

    while out.ends_with('-') {
        out.pop();
    }

    if out.is_empty() {
        "untitled".to_string()
    } else {
        out
    }
}

/// Split a Markdown document with `---` YAML frontmatter into `(yaml_str, body_str)`.
pub fn split_frontmatter(content: &str) -> Result<(&str, &str)> {
    let bytes = content.as_bytes();
    let mut pos = 0;

    // Skip optional UTF-8 BOM.
    if content.starts_with('\u{feff}') {
        pos += '\u{feff}'.len_utf8();
    }

    let rest = &content[pos..];
    let after_open = if rest.starts_with("---\r\n") {
        pos + 5
    } else if rest.starts_with("---\n") {
        pos + 4
    } else {
        return Err(VaultError::InvalidInput(
            "Markdown entry must begin with YAML frontmatter fence `---`".to_string(),
        ));
    };

    let yaml_start = after_open;
    let mut cursor = yaml_start;
    while cursor <= bytes.len() {
        let line_end = content[cursor..]
            .find('\n')
            .map(|rel| cursor + rel)
            .unwrap_or(bytes.len());
        let line = content[cursor..line_end].trim_end();
        if line == "---" || line == "..." {
            let yaml = &content[yaml_start..cursor];
            let mut body_start = if line_end < bytes.len() {
                line_end + 1
            } else {
                line_end
            };
            // Skip the single canonical blank separator line after closing `---`.
            if let Some(after_cr) = content[body_start..].strip_prefix("\r\n") {
                let _ = after_cr;
                body_start += 2;
            } else if content[body_start..].starts_with('\n') {
                body_start += 1;
            }
            return Ok((yaml, &content[body_start..]));
        }
        if line_end == bytes.len() {
            break;
        }
        cursor = line_end + 1;
    }

    Err(VaultError::InvalidInput(
        "Unterminated YAML frontmatter block (missing closing `---`)".to_string(),
    ))
}

/// Parse a `.md` vault file with YAML frontmatter into an [`Entry`] without rendering HTML.
pub fn parse_entry_file_no_html(content: &str, rel_path: &str) -> Result<Entry> {
    let (yaml, body) = split_frontmatter(content)?;
    let mut frontmatter: EntryFrontmatter = serde_yaml::from_str(yaml)?;

    if frontmatter.links.is_empty() {
        frontmatter.links = extract_wikilinks(body);
    }

    Ok(Entry::new(frontmatter, body, rel_path))
}

/// Parse a `.md` vault file with YAML frontmatter into an [`Entry`] with rendered `html_preview`.
pub fn parse_entry_file(content: &str, rel_path: &str) -> Result<Entry> {
    let mut entry = parse_entry_file_no_html(content, rel_path)?;
    entry.html_preview = Some(render_html_preview(&entry.body));
    Ok(entry)
}

/// Serialize [`EntryFrontmatter`] and Markdown body into a canonical UTF-8 `.md` document.
pub fn serialize_entry(frontmatter: &EntryFrontmatter, body: &str) -> Result<String> {
    let raw_yaml = serde_yaml::to_string(frontmatter)?;
    let yaml = raw_yaml
        .strip_prefix("---\r\n")
        .or_else(|| raw_yaml.strip_prefix("---\n"))
        .unwrap_or(&raw_yaml)
        .trim_end_matches(['\r', '\n']);

    Ok(format!("---\n{yaml}\n---\n\n{body}"))
}

/// Extract deduplicated outgoing `[[target|label]]` / `[[target]]` wiki-link targets from Markdown.
pub fn extract_wikilinks(markdown: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    let re = wikilink_regex();

    let mut text_buf = String::new();
    let flush_buf = |buf: &mut String, targets: &mut Vec<String>, seen: &mut HashSet<String>| {
        if buf.is_empty() {
            return;
        }
        for cap in re.captures_iter(buf) {
            let raw = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let target = raw.split('|').next().unwrap_or("").trim();
            if !target.is_empty() {
                let key = target.to_ascii_lowercase();
                if seen.insert(key) {
                    targets.push(target.to_string());
                }
            }
        }
        buf.clear();
    };

    let mut in_code_block = false;
    for event in Parser::new_ext(markdown, markdown_options()) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                flush_buf(&mut text_buf, &mut targets, &mut seen);
                in_code_block = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
            }
            Event::Text(text) if !in_code_block => text_buf.push_str(&text),
            _ => flush_buf(&mut text_buf, &mut targets, &mut seen),
        }
    }
    flush_buf(&mut text_buf, &mut targets, &mut seen);

    targets
}

/// Render Markdown prose (including `[[wikilink]]` anchors) to HTML via `pulldown-cmark`.
pub fn render_html_preview(markdown: &str) -> String {
    let re = wikilink_regex();
    let parser = Parser::new_ext(markdown, markdown_options());

    let mut events: Vec<Event<'_>> = Vec::new();
    let mut pending_text = String::new();

    let flush_text = |buf: &mut String, out: &mut Vec<Event<'_>>| {
        if buf.is_empty() {
            return;
        }
        if !buf.contains("[[") {
            out.push(Event::Text(std::mem::take(buf).into()));
            return;
        }
        let mut last = 0;
        for cap in re.captures_iter(buf) {
            let full = cap.get(0).expect("full match");
            if full.start() > last {
                out.push(Event::Text(buf[last..full.start()].to_string().into()));
            }
            let inner = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let mut parts = inner.splitn(2, '|');
            let target = parts.next().unwrap_or("").trim();
            let label = parts
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(target);

            if target.is_empty() {
                out.push(Event::Text(full.as_str().to_string().into()));
            } else {
                let escaped_target = escape_html_attr(target);
                let escaped_href = percent_encode_fragment(target);
                let escaped_label = escape_html_text(label);
                let html = format!(
                    "<a class=\"wikilink\" data-target=\"{escaped_target}\" href=\"#/entry/{escaped_href}\">{escaped_label}</a>"
                );
                out.push(Event::InlineHtml(html.into()));
            }
            last = full.end();
        }
        if last < buf.len() {
            out.push(Event::Text(buf[last..].to_string().into()));
        }
        buf.clear();
    };

    let mut in_code_block = false;
    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(_)) => {
                flush_text(&mut pending_text, &mut events);
                in_code_block = true;
                events.push(event);
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                events.push(event);
            }
            Event::Text(text) if !in_code_block => pending_text.push_str(&text),
            Event::Html(raw) | Event::InlineHtml(raw) => {
                flush_text(&mut pending_text, &mut events);
                events.push(Event::Text(raw));
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                flush_text(&mut pending_text, &mut events);
                let safe_url = sanitize_markdown_url(&dest_url);
                events.push(Event::Start(Tag::Link {
                    link_type,
                    dest_url: safe_url.into(),
                    title,
                    id,
                }));
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                flush_text(&mut pending_text, &mut events);
                let safe_url = sanitize_markdown_url(&dest_url);
                events.push(Event::Start(Tag::Image {
                    link_type,
                    dest_url: safe_url.into(),
                    title,
                    id,
                }));
            }
            other => {
                flush_text(&mut pending_text, &mut events);
                events.push(other);
            }
        }
    }
    flush_text(&mut pending_text, &mut events);

    let mut html_out = String::new();
    html::push_html(&mut html_out, events.into_iter());
    html_out
}

fn markdown_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
}

/// Strip control characters and reject unsafe URL schemes in Markdown links and images.
fn sanitize_markdown_url(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .collect();
    if let Some(colon) = cleaned.find(':') {
        let before_colon = &cleaned[..colon];
        // Only treat as a URL scheme if `:` precedes any path/query/fragment delimiter.
        if !before_colon.contains(['/', '?', '#']) {
            let scheme = before_colon.to_ascii_lowercase();
            if !matches!(
                scheme.as_str(),
                "http" | "https" | "mailto" | "vault" | "vault-media"
            ) {
                return "#blocked".to_string();
            }
        }
    }
    raw.to_string()
}

pub(crate) fn escape_html_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_html_attr(s: &str) -> String {
    escape_html_text(s)
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn percent_encode_fragment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}
