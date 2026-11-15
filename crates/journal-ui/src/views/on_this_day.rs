//! Archival "On This Day" memory spread with engraved rotary postmark badges.

use chrono::Datelike;
use journal_types::OnThisDayGroup;
use leptos::prelude::*;
use uuid::Uuid;

use crate::icons::{
    IconClockHistory, IconMapPin, IconMic, IconStar, WeatherGlyph,
};
use crate::views::format_entry_date;

/// Historical memory spread displaying past entries from the same calendar month & day.
#[component]
pub fn OnThisDayView(
    groups: Signal<Vec<OnThisDayGroup>>,
    on_open_entry: Callback<Uuid>,
) -> impl IntoView {
    let today_label = chrono::Utc::now().format("%B %d").to_string();

    view! {
        <section class="folio-stage on-this-day-stage" aria-label="On This Day Memories">
            <header class="stage-header">
                <div>
                    <span class="small-caps-meta">{format!("Archival Echoes · {today_label}")}</span>
                    <h1 class="stage-serif-title">"On This Day"</h1>
                </div>
                <div class="postmark-header-seal" aria-hidden="true">
                    <img src="/assets/svg/postmark-circle.svg" alt="" />
                </div>
            </header>

            {move || {
                let list = groups.get();
                if list.is_empty() {
                    return view! {
                        <div class="editorial-empty-card">
                            <IconClockHistory class="empty-emblem" />
                            <h2 class="empty-serif-heading">"No Past Echoes for This Date Yet"</h2>
                            <p class="empty-prose-note">
                                "As seasons turn, folios written on this calendar day in prior years will gather here like letters returned to the desk."
                            </p>
                        </div>
                    }
                        .into_any();
                }

                view! {
                    <div class="on-this-day-groups">
                        {list
                            .into_iter()
                            .map(|group| {
                                let years_badge = if group.years_ago == 1 {
                                    "1 YEAR AGO".to_string()
                                } else {
                                    format!("{} YEARS AGO", group.years_ago)
                                };
                                let year_num = group.year;

                                view! {
                                    <section class="otd-year-section">
                                        <div class="otd-postmark-row">
                                            <div class="rotary-postmark-badge">
                                                <span class="rotary-ring-text">{years_badge}</span>
                                                <span class="rotary-year-num">{year_num}</span>
                                            </div>
                                            <span class="timeline-month-rule"></span>
                                        </div>

                                        <div class="otd-cards-grid">
                                            {group
                                                .entries
                                                .into_iter()
                                                .map(|entry| {
                                                    let entry_id = entry.id;
                                                    let title = if entry.title.trim().is_empty() {
                                                        "Untitled Folio".to_string()
                                                    } else {
                                                        entry.title.clone()
                                                    };
                                                    let date_str = format_entry_date(&entry.date);
                                                    let day_num = entry.date.day();
                                                    let cover_photo = entry.cover_photo.clone();
                                                    let snippet = entry.snippet.clone();
                                                    let location = entry.location.clone();
                                                    let weather = entry.weather.clone();
                                                    let starred = entry.starred;
                                                    let has_audio = entry.has_audio;

                                                    view! {
                                                        <article
                                                            class="otd-memory-card"
                                                            on:click=move |_| on_open_entry.run(entry_id)
                                                        >
                                                            <div class="otd-card-header">
                                                                <span class="small-caps-meta">{format!("{date_str} · Day {day_num}")}</span>
                                                                <div class="otd-card-badges">
                                                                    {has_audio.then(|| view! { <IconMic class="tiny-icon" /> })}
                                                                    {starred.then(|| view! { <IconStar filled=true class="tiny-icon" /> })}
                                                                </div>
                                                            </div>

                                                            <div class="otd-card-body">
                                                                <div class="otd-card-prose">
                                                                    <h3 class="timeline-card-title">{title}</h3>
                                                                    <div class="timeline-card-meta-row">
                                                                        {location.map(|loc| {
                                                                            view! {
                                                                                <span class="meta-chip">
                                                                                    <IconMapPin class="tiny-icon" />
                                                                                    <span>{loc.place_name}</span>
                                                                                </span>
                                                                            }
                                                                        })}
                                                                        {weather.map(|w| {
                                                                            view! {
                                                                                <span class="meta-chip">
                                                                                    <WeatherGlyph icon_key=w.icon_key class="tiny-icon" />
                                                                                    <span>{w.condition}</span>
                                                                                </span>
                                                                            }
                                                                        })}
                                                                    </div>
                                                                    <p class="timeline-card-excerpt">{snippet}</p>
                                                                </div>

                                                                {cover_photo.map(|src| {
                                                                    view! {
                                                                        <div class="otd-polaroid-inset">
                                                                            <img src=src alt="Memory Polaroid" loading="lazy" />
                                                                        </div>
                                                                    }
                                                                })}
                                                            </div>
                                                        </article>
                                                    }
                                                })
                                                .collect_view()}
                                        </div>
                                    </section>
                                }
                            })
                            .collect_view()}
                    </div>
                }
                    .into_any()
            }}
        </section>
    }
}
