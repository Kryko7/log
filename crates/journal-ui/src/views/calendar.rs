//! 7-column botanical Calendar Mosaic view with Polaroid miniatures and entry pips.

use std::collections::HashMap;

use chrono::{Datelike, NaiveDate, Utc};
use journal_types::{CalendarDaySummary, EntrySummary};
use leptos::prelude::*;

use crate::icons::{
    IconCamera, IconChevronLeft, IconChevronRight, IconMic, IconStar,
};
use crate::views::{days_in_month, first_weekday_sunday0, format_month_year};

const WEEKDAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

/// Monthly botanical mosaic grid displaying entry counts, Polaroid miniatures, and starred markers.
#[component]
pub fn CalendarMosaicView(
    year: Signal<i32>,
    month: Signal<u32>,
    days: Signal<Vec<CalendarDaySummary>>,
    #[prop(optional)] entries: Option<Signal<Vec<EntrySummary>>>,
    on_change_month: Callback<(i32, u32)>,
    on_select_date: Callback<NaiveDate>,
) -> impl IntoView {
    let today = Utc::now().date_naive();

    let go_prev = move |_| {
        let y = year.get_untracked();
        let m = month.get_untracked();
        if m <= 1 {
            on_change_month.run((y - 1, 12));
        } else {
            on_change_month.run((y, m - 1));
        }
    };

    let go_next = move |_| {
        let y = year.get_untracked();
        let m = month.get_untracked();
        if m >= 12 {
            on_change_month.run((y + 1, 1));
        } else {
            on_change_month.run((y, m + 1));
        }
    };

    view! {
        <section class="folio-stage calendar-stage" aria-label="Calendar Mosaic">
            <header class="stage-header calendar-stage-header">
                <div>
                    <span class="small-caps-meta">"Calendar Mosaic"</span>
                    <h1 class="stage-serif-title">{move || format_month_year(year.get(), month.get())}</h1>
                </div>

                <div class="calendar-nav-controls">
                    <button
                        type="button"
                        class="btn-editorial btn-ghost"
                        title="Previous month"
                        on:click=go_prev
                    >
                        <IconChevronLeft />
                        <span>"Prev"</span>
                    </button>
                    <button
                        type="button"
                        class="btn-editorial btn-ghost"
                        title="Jump to current month"
                        on:click=move |_| on_change_month.run((today.year(), today.month()))
                    >
                        <span>"Today"</span>
                    </button>
                    <button
                        type="button"
                        class="btn-editorial btn-ghost"
                        title="Next month"
                        on:click=go_next
                    >
                        <span>"Next"</span>
                        <IconChevronRight />
                    </button>
                </div>
            </header>

            <div class="calendar-mosaic-sheet">
                <div class="calendar-weekday-row">
                    {WEEKDAYS
                        .iter()
                        .map(|d| view! { <div class="calendar-weekday-cell small-caps-meta">{*d}</div> })
                        .collect_view()}
                </div>

                <div class="calendar-grid">
                    {move || {
                        let y = year.get();
                        let m = month.get();
                        let leading = first_weekday_sunday0(y, m);
                        let total_days = days_in_month(y, m);

                        let day_map: HashMap<NaiveDate, CalendarDaySummary> = days
                            .get()
                            .into_iter()
                            .map(|d| (d.date, d))
                            .collect();

                        let photo_map: HashMap<NaiveDate, String> = entries
                            .map(|sig| {
                                sig.get()
                                    .into_iter()
                                    .filter_map(|e| e.cover_photo.map(|p| (e.date.date_naive(), p)))
                                    .collect()
                            })
                            .unwrap_or_default();

                        let blanks = (0..leading)
                            .map(|_| view! { <div class="calendar-day-cell is-pad"></div> }.into_any());

                        let day_cells = (1..=total_days).map(|day_num| {
                            let Some(date) = NaiveDate::from_ymd_opt(y, m, day_num) else {
                                return view! { <div class="calendar-day-cell is-pad"></div> }.into_any();
                            };
                            let summary = day_map.get(&date).cloned();
                            let thumb = photo_map.get(&date).cloned();
                            let has_thumb = thumb.is_some();
                            let is_today = date == today;
                            let entry_count = summary.as_ref().map(|s| s.entry_count).unwrap_or(0);
                            let has_entries = entry_count > 0;
                            let starred = summary.as_ref().map(|s| s.starred_count > 0).unwrap_or(false);
                            let has_audio = summary.as_ref().map(|s| s.has_audio).unwrap_or(false);
                            let has_photos = summary.as_ref().map(|s| s.has_photos).unwrap_or(false) || has_thumb;
                            let mood_label = summary.as_ref().and_then(|s| s.moods.first().cloned());

                            view! {
                                <button
                                    type="button"
                                    class="calendar-day-cell"
                                    class:has-entries=has_entries
                                    class:is-today=is_today
                                    class:has-photo=has_thumb
                                    on:click=move |_| on_select_date.run(date)
                                >
                                    {thumb.map(|src| {
                                        view! {
                                            <div class="calendar-day-thumb-bg">
                                                <img src=src alt="" loading="lazy" />
                                                <div class="calendar-day-thumb-scrim"></div>
                                            </div>
                                        }
                                    })}

                                    <div class="calendar-day-top">
                                        <span class="calendar-day-number">{day_num}</span>
                                        {starred.then(|| {
                                            view! {
                                                <span class="calendar-day-star">
                                                    <IconStar filled=true class="tiny-icon" />
                                                </span>
                                            }
                                        })}
                                    </div>

                                    <div class="calendar-day-bottom">
                                        {mood_label.map(|m_txt| {
                                            view! { <span class="calendar-mood-pill">{m_txt}</span> }
                                        })}

                                        <div class="calendar-day-indicators">
                                            {has_photos.then(|| view! { <IconCamera class="tiny-icon" /> })}
                                            {has_audio.then(|| view! { <IconMic class="tiny-icon" /> })}
                                            {(entry_count > 0).then(|| {
                                                let pips = entry_count.min(4);
                                                view! {
                                                    <span class="calendar-pips" title=format!("{entry_count} entries")>
                                                        {(0..pips).map(|_| view! { <span class="calendar-pip"></span> }).collect_view()}
                                                    </span>
                                                }
                                            })}
                                        </div>
                                    </div>
                                </button>
                            }
                                .into_any()
                        });

                        blanks.chain(day_cells).collect_view()
                    }}
                </div>
            </div>
        </section>
    }
}
