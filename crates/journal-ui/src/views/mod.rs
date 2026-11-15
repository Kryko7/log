//! Primary spread views and tactile scrapbook components for **Log**.

pub mod calendar;
pub mod modals;
pub mod on_this_day;
pub mod polaroid;
pub mod stamps;
pub mod timeline;

pub use calendar::CalendarMosaicView;
pub use modals::{BackupSyncModal, MigrationModal};
pub use on_this_day::OnThisDayView;
pub use polaroid::PolaroidCard;
pub use stamps::{StampPickerPopover, StampStickerLayer};
pub use timeline::TimelineView;

use chrono::{DateTime, Datelike, NaiveDate, Utc};

/// Top-level route view enum kept compatible with the workspace skeleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveView {
    Journal,
    Calendar,
    OnThisDay,
    Archive,
    Settings,
}

/// Removes inline Markdown image tags `![alt](url)` from a string.
pub fn strip_markdown_images(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("![") {
        out.push_str(&rest[..start]);
        let after_bang = &rest[start + 2..];
        if let Some(mid) = after_bang.find("](") {
            let after_paren = &after_bang[mid + 2..];
            if let Some(end) = after_paren.find(')') {
                if !after_bang[..mid].contains('\n') && !after_paren[..end].contains('\n') {
                    rest = &after_paren[end + 1..];
                    continue;
                }
            }
        }
        out.push_str("![");
        rest = after_bang;
    }
    out.push_str(rest);
    out
}

/// Cleans an entry excerpt for timeline / memory rows by removing Markdown image
/// syntax and collapsing whitespace cleanly.
pub fn clean_excerpt_snippet(raw: &str) -> String {
    strip_markdown_images(raw)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_prose_prefix(input: &str) -> String {
    let without_images = strip_markdown_images(input);
    let mut cleaned = String::with_capacity(without_images.len());
    let mut prev_space = true;
    for line in without_images.lines() {
        let trimmed = line
            .trim()
            .trim_start_matches(|c: char| matches!(c, '#' | '*' | '_' | '>' | '-'));
        for ch in trimmed.chars() {
            if ch.is_whitespace() {
                if !prev_space {
                    cleaned.push(' ');
                    prev_space = true;
                }
            } else if !matches!(ch, '*' | '_' | '`') {
                for lc in ch.to_lowercase() {
                    cleaned.push(lc);
                }
                prev_space = false;
            }
        }
        if !prev_space {
            cleaned.push(' ');
            prev_space = true;
        }
    }
    cleaned.trim().to_string()
}

/// Returns `true` when an entry title is blank, a generic placeholder, an image
/// tag artifact, or was auto-derived by truncating the opening sentence of the
/// entry body/snippet (as happens in Day One imports without explicit titles).
pub fn is_auto_derived_title(title: &str, body_or_snippet: &str) -> bool {
    let raw = title.trim();
    if raw.is_empty()
        || raw.eq_ignore_ascii_case("untitled entry")
        || raw.eq_ignore_ascii_case("untitled folio")
        || (raw.starts_with("![") && raw.contains("]("))
    {
        return true;
    }

    let stripped_title = raw
        .trim_end_matches('…')
        .trim_end_matches("...")
        .trim();
    let norm_title = normalize_prose_prefix(stripped_title);
    if norm_title.chars().count() < 10 {
        return false;
    }

    let norm_body = normalize_prose_prefix(body_or_snippet);
    if norm_body.is_empty() {
        return false;
    }

    if norm_body.starts_with(&norm_title) {
        return true;
    }

    // Handle cases where Day One truncated mid-word before appending ellipsis.
    let prefix: String = norm_title.chars().take(24).collect();
    prefix.chars().count() >= 10 && norm_body.starts_with(&prefix)
}

/// Format an entry UTC timestamp as a concise editorial date string (e.g. `Nov 15, 2026`).
pub fn format_entry_date(dt: &DateTime<Utc>) -> String {
    dt.format("%b %d, %Y").to_string()
}

/// Format a month & year heading (e.g. `November 2026`).
pub fn format_month_year(year: i32, month: u32) -> String {
    let name = match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "Month",
    };
    format!("{name} {year}")
}

/// Format a byte count into a clean human-readable label.
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Compute the number of days in `(year, month)`.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_y, next_m) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let Some(first) = NaiveDate::from_ymd_opt(year, month, 1) else {
        return 30;
    };
    let Some(next_first) = NaiveDate::from_ymd_opt(next_y, next_m, 1) else {
        return 30;
    };
    (next_first - first).num_days().max(28) as u32
}

/// Weekday index (`0 = Sunday .. 6 = Saturday`) of the 1st day of `(year, month)`.
pub fn first_weekday_sunday0(year: i32, month: u32) -> u32 {
    NaiveDate::from_ymd_opt(year, month, 1)
        .map(|d| d.weekday().num_days_from_sunday())
        .unwrap_or(0)
}
