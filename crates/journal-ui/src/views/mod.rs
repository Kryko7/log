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
