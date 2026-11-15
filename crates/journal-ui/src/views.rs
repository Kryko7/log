//! Primary spread views: Timeline, Calendar, On This Day, Archive, Migration, Backup.

/// Top-level route view enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveView {
    Journal,
    Calendar,
    OnThisDay,
    Archive,
    Settings,
}
