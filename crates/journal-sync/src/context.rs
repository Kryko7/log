//! Daily writing context cues: Open-Meteo weather (with 2.5s timeout + offline fallback),
//! read-only Google Calendar event prompts, and On This Day vault counts.

use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use journal_core::Vault;
use journal_types::{CalendarEventPrompt, DailyContext, WeatherInfo};
use serde::Deserialize;

use crate::error::{Result, SyncError};

const DEFAULT_LAT: f64 = 6.9271;
const DEFAULT_LNG: f64 = 79.8612;
const OPEN_METEO_URL: &str = "https://api.open-meteo.com/v1/forecast";
const GCAL_EVENTS_URL: &str = "https://www.googleapis.com/calendar/v3/calendars/primary/events";

#[derive(Debug, Deserialize)]
struct OpenMeteoResponse {
    current: Option<OpenMeteoCurrent>,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoCurrent {
    temperature_2m: Option<f32>,
    relative_humidity_2m: Option<f32>,
    weather_code: Option<i32>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CalendarPayload {
    PromptList(Vec<CalendarEventPrompt>),
    RawList(Vec<RawCalendarEvent>),
    ItemsWrapper {
        #[serde(default)]
        items: Vec<RawCalendarEvent>,
    },
    EventsWrapper {
        #[serde(default)]
        events: Vec<RawCalendarEvent>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCalendarEvent {
    #[serde(default)]
    id: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    start: Option<RawEventDateTime>,
    #[serde(default)]
    end: Option<RawEventDateTime>,
    #[serde(default)]
    organizer: Option<RawOrganizer>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEventDateTime {
    #[serde(default)]
    date_time: Option<String>,
    #[serde(default)]
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOrganizer {
    #[serde(default)]
    display_name: Option<String>,
}

/// Assemble private, read-only daily writing cues for `date`:
/// - `weather_hint` from Open-Meteo (defaulting to Colombo `6.9271, 79.8612`,
///   2.5s timeout with graceful offline fallback).
/// - `events` from `~/.cache/log-journal/today-events.json` (or `LOG_CALENDAR_CACHE_PATH`)
///   or Google Calendar v3 REST API (`LOG_GCAL_TOKEN`), transformed into literary prompts.
/// - `on_this_day_count` queried from `vault`.
pub async fn get_daily_context(
    vault: &Vault,
    date: NaiveDate,
    lat_opt: Option<f64>,
    lng_opt: Option<f64>,
) -> Result<DailyContext> {
    let lat = lat_opt.unwrap_or(DEFAULT_LAT);
    let lng = lng_opt.unwrap_or(DEFAULT_LNG);

    let on_this_day_groups = vault
        .get_on_this_day(date.month(), date.day(), date.year())
        .map_err(|e| SyncError::Vault(e.to_string()))?;
    let on_this_day_count: usize = on_this_day_groups.iter().map(|g| g.entries.len()).sum();

    let offline = std::env::var("LOG_OFFLINE")
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);

    let weather_hint = if offline {
        None
    } else {
        fetch_open_meteo_weather(lat, lng).await
    };

    let mut events = load_cached_calendar_events(date);
    if events.is_empty() && !offline {
        if let Ok(token) = std::env::var("LOG_GCAL_TOKEN") {
            let token = token.trim();
            if !token.is_empty() {
                events = fetch_gcal_events(token, date).await;
            }
        }
    }

    let mut suggested_prompts: Vec<String> = events
        .iter()
        .map(|ev| ev.suggested_prompt.clone())
        .collect();

    if on_this_day_count > 0 {
        let noun = if on_this_day_count == 1 {
            "entry"
        } else {
            "entries"
        };
        suggested_prompts.push(format!(
            "You wrote {on_this_day_count} {noun} on this date in past years — what feels different now?"
        ));
    }

    if suggested_prompts.is_empty() {
        suggested_prompts.push(
            "What small, quiet detail from today would you like to remember a year from now?"
                .to_string(),
        );
        suggested_prompts.push(
            "How did the light, weather, and pace of the day shape your thoughts?".to_string(),
        );
    }

    Ok(DailyContext {
        date,
        events,
        on_this_day_count,
        suggested_prompts,
        weather_hint,
    })
}

async fn fetch_open_meteo_weather(lat: f64, lng: f64) -> Option<WeatherInfo> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(1500))
        .timeout(Duration::from_millis(2500))
        .build()
        .ok()?;

    let url = format!(
        "{OPEN_METEO_URL}?latitude={lat:.4}&longitude={lng:.4}&current=temperature_2m,relative_humidity_2m,weather_code"
    );

    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let parsed: OpenMeteoResponse = resp.json().await.ok()?;
    let current = parsed.current?;

    let (condition, icon_key) = map_wmo_weather_code(current.weather_code.unwrap_or(-1));
    let temperature_c = current.temperature_2m;
    let humidity_pct = current
        .relative_humidity_2m
        .map(|h| h.round().clamp(0.0, 100.0) as u8);
    let summary = temperature_c.map(|t| format!("{condition}, {t:.1}°C"));

    Some(WeatherInfo {
        condition: condition.to_string(),
        temperature_c,
        icon_key: icon_key.to_string(),
        humidity_pct,
        summary,
    })
}

/// Map a WMO weather interpretation code to a human condition label and fine SVG `icon_key`.
pub fn map_wmo_weather_code(code: i32) -> (&'static str, &'static str) {
    match code {
        0 => ("Clear Sky", "clear-day"),
        1 | 2 => ("Partly Cloudy", "partly-cloudy"),
        3 => ("Overcast", "cloudy"),
        45 | 48 => ("Misty Fog", "fog"),
        51 | 53 | 55 | 56 | 57 => ("Light Drizzle", "drizzle"),
        61 | 63 | 65 | 66 | 67 | 80 | 81 | 82 => ("Rain", "rain"),
        71 | 73 | 75 | 77 | 85 | 86 => ("Snow", "snow"),
        95 | 96 | 99 => ("Thunderstorm", "thunderstorm"),
        _ => ("Mild Weather", "partly-cloudy"),
    }
}

fn calendar_cache_path() -> PathBuf {
    if let Ok(override_path) = std::env::var("LOG_CALENDAR_CACHE_PATH") {
        let trimmed = override_path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cache")
        .join("log-journal")
        .join("today-events.json")
}

fn load_cached_calendar_events(date: NaiveDate) -> Vec<CalendarEventPrompt> {
    let path = calendar_cache_path();
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_calendar_json(&raw, date)
}

async fn fetch_gcal_events(bearer_token: &str, date: NaiveDate) -> Vec<CalendarEventPrompt> {
    let Ok(client) = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(1500))
        .timeout(Duration::from_millis(2500))
        .build()
    else {
        return Vec::new();
    };

    let time_min = format!("{}T00:00:00Z", date.format("%Y-%m-%d"));
    let time_max = format!("{}T23:59:59Z", date.format("%Y-%m-%d"));

    let Ok(resp) = client
        .get(GCAL_EVENTS_URL)
        .bearer_auth(bearer_token)
        .query(&[
            ("timeMin", time_min.as_str()),
            ("timeMax", time_max.as_str()),
            ("singleEvents", "true"),
            ("orderBy", "startTime"),
            ("maxResults", "50"),
        ])
        .send()
        .await
    else {
        return Vec::new();
    };

    if !resp.status().is_success() {
        return Vec::new();
    }
    let Ok(raw) = resp.text().await else {
        return Vec::new();
    };
    parse_calendar_json(&raw, date)
}

/// Parse `gcalendar readonly events --json` or Google Calendar v3 JSON into reflection prompts.
pub fn parse_calendar_json(raw: &str, target_date: NaiveDate) -> Vec<CalendarEventPrompt> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let Ok(payload) = serde_json::from_str::<CalendarPayload>(trimmed) else {
        return Vec::new();
    };

    match payload {
        CalendarPayload::PromptList(list) => list,
        CalendarPayload::RawList(events) => convert_raw_events(events, target_date),
        CalendarPayload::ItemsWrapper { items } => convert_raw_events(items, target_date),
        CalendarPayload::EventsWrapper { events } => convert_raw_events(events, target_date),
    }
}

fn convert_raw_events(
    events: Vec<RawCalendarEvent>,
    target_date: NaiveDate,
) -> Vec<CalendarEventPrompt> {
    let mut prompts = Vec::new();
    for ev in events {
        let title = ev
            .summary
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Calendar Moment")
            .to_string();

        let (start_date, start_time, start_hour) = ev
            .start
            .as_ref()
            .map(extract_event_date_and_time)
            .unwrap_or((None, None, None));

        if let Some(ev_date) = start_date {
            if ev_date != target_date {
                continue;
            }
        }

        let (_, end_time, _) = ev
            .end
            .as_ref()
            .map(extract_event_date_and_time)
            .unwrap_or((None, None, None));

        let location = ev
            .location
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let calendar_name = ev
            .organizer
            .and_then(|o| o.display_name)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let suggested_prompt = build_literary_prompt(&title, start_hour, location.as_deref());

        let id = if ev.id.trim().is_empty() {
            format!("evt-{}", prompts.len() + 1)
        } else {
            ev.id
        };

        prompts.push(CalendarEventPrompt {
            id,
            title,
            start_time,
            end_time,
            location,
            calendar_name,
            suggested_prompt,
        });
    }
    prompts
}

fn extract_event_date_and_time(
    dt: &RawEventDateTime,
) -> (Option<NaiveDate>, Option<String>, Option<u32>) {
    if let Some(raw_dt) = dt
        .date_time
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Ok(parsed) = DateTime::parse_from_rfc3339(raw_dt) {
            let local_naive = parsed.naive_local();
            return (
                Some(local_naive.date()),
                Some(local_naive.format("%H:%M").to_string()),
                Some(local_naive.hour()),
            );
        }
        if let Ok(naive) = NaiveDateTime::parse_from_str(raw_dt, "%Y-%m-%dT%H:%M:%S") {
            return (
                Some(naive.date()),
                Some(naive.format("%H:%M").to_string()),
                Some(naive.hour()),
            );
        }
    }
    if let Some(raw_d) = dt.date.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if let Ok(d) = NaiveDate::parse_from_str(raw_d, "%Y-%m-%d") {
            return (Some(d), None, None);
        }
        if let Ok(t) = NaiveTime::parse_from_str(raw_d, "%H:%M") {
            return (None, Some(t.format("%H:%M").to_string()), Some(t.hour()));
        }
    }
    (None, None, None)
}

fn build_literary_prompt(title: &str, hour: Option<u32>, location: Option<&str>) -> String {
    let period = match hour {
        Some(0..=11) => "this morning",
        Some(12..=16) => "this afternoon",
        Some(17..=23) => "this evening",
        _ => "today",
    };
    match location {
        Some(loc) => {
            format!("Reflect on '{title}' at {loc} from {period} — what details stayed with you?")
        }
        None => format!("Reflect on '{title}' from {period} — what moment stayed with you?"),
    }
}
