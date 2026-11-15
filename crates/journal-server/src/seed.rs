//! Showcase vault seeder (`log-journal seed-demo`) with genuine literary prose,
//! synthesized Polaroid photography, playable harmonic voice memo, and bidirectional wikilinks.

use anyhow::Result;
use chrono::{Datelike, Days, NaiveDate, TimeZone, Utc};
use journal_core::Vault;
use journal_types::{
    CreateNotebookRequest, EntryFrontmatter, GeoLocation, PolaroidMeta, StampPlacement,
    WeatherInfo,
};
use uuid::Uuid;

use crate::assets::resolve_repo_path;
use crate::media_gen::{generate_acoustic_chord_wav, generate_veranda_polaroid_png};

/// Summary returned after seeding the showcase vault.
#[derive(Debug, Clone)]
pub struct SeedSummary {
    pub vault_path: String,
    pub notebooks_created: usize,
    pub entries_seeded: usize,
}

/// Populate `vault` with an illustrative editorial showcase (2 notebooks, 4 literary
/// entries across two notebooks, bidirectional wikilinks, a Polaroid photo, a playable
/// 3-second acoustic `.wav` voice memo, die-cut botanical stamps, and an "On This Day"
/// entry from exactly one year ago).
pub fn seed_demo_vault(vault: &Vault) -> Result<SeedSummary> {
    let mut notebooks_created = 0usize;
    let existing_notebooks = vault.list_notebooks()?;

    // Ensure "Field & Botanical Notes" exists alongside default "Daily Sanctuary".
    let botanical_nb_id = if let Some(nb) = existing_notebooks
        .iter()
        .find(|n| n.name.eq_ignore_ascii_case("Field & Botanical Notes"))
    {
        nb.id.clone()
    } else {
        let nb = vault.create_notebook(CreateNotebookRequest {
            name: "Field & Botanical Notes".to_string(),
            description: Some(
                "Pressed leaves, monsoon specimen sketches, and unhurried botanical observations."
                    .to_string(),
            ),
            cover_asset: Some("/assets/covers/pressed-wildflowers.png".to_string()),
            accent_color: Some("#546E5A".to_string()),
        })?;
        notebooks_created += 1;
        nb.id
    };

    let existing_entries = vault.list_entries(None, None, Some(500), None)?;
    let has_title = |title: &str| {
        existing_entries
            .iter()
            .any(|e| e.title.eq_ignore_ascii_case(title))
    };

    let today = Utc::now().date_naive();
    let one_year_ago = NaiveDate::from_ymd_opt(today.year() - 1, today.month(), today.day())
        .or_else(|| NaiveDate::from_ymd_opt(today.year() - 1, today.month(), 28))
        .unwrap_or(today);
    let nov_first = if today.day() > 1 {
        NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap_or(today)
    } else {
        today
            .checked_sub_days(Days::new(14))
            .unwrap_or(today)
    };
    let herbarium_day = today
        .checked_sub_days(Days::new(6))
        .unwrap_or(today);

    let mut entries_seeded = 0usize;

    // -----------------------------------------------------------------------
    // Entry 1 (Today): "Rain on the Teak Sill & Unhurried Ink"
    // -----------------------------------------------------------------------
    let title_1 = "Rain on the Teak Sill & Unhurried Ink";
    if !has_title(title_1) {
        let photo_bytes = generate_veranda_polaroid_png()?;
        let mut veranda_photo = vault.ingest_photo(
            "monsoon-veranda.png",
            &photo_bytes,
            Some("Monsoon light across the veranda".to_string()),
        )?;
        veranda_photo.rotation_deg = -1.8;
        veranda_photo.washi_asset = Some("/assets/svg/washi-terracotta.svg".to_string());
        veranda_photo.location = Some(colombo_location());

        let wav_bytes = generate_acoustic_chord_wav()?;
        let mut voice_memo = vault.ingest_audio(
            "veranda-evening-chord.wav",
            &wav_bytes,
            Some(3.0),
            Some("audio/wav".to_string()),
        )?;
        voice_memo.transcript = Some(
            "Three-second veranda chord before the evening downpour settled in.".to_string(),
        );

        let dt1 = Utc
            .from_utc_datetime(&today.and_hms_opt(9, 20, 0).expect("valid time"));

        let mut fm1 = EntryFrontmatter::new(title_1);
        fm1.id = Uuid::now_v7();
        fm1.date = dt1;
        fm1.updated_at = dt1;
        fm1.notebook_id = Some("daily".to_string());
        fm1.tags = vec![
            "monsoon".to_string(),
            "veranda".to_string(),
            "fountain-pen".to_string(),
        ];
        fm1.mood = Some("reflective".to_string());
        fm1.starred = true;
        fm1.weather = Some(WeatherInfo {
            condition: "Light Rain".to_string(),
            temperature_c: Some(24.0),
            icon_key: "rain".to_string(),
            humidity_pct: Some(84),
            summary: Some("Warm silver drizzle drifting across the terracotta eaves".to_string()),
        });
        fm1.location = Some(colombo_location());
        fm1.photos = vec![veranda_photo];
        fm1.audio = vec![voice_memo];
        fm1.stamps = vec![
            StampPlacement {
                id: "seed-ginkgo-1".to_string(),
                asset_id: "pressed-ginkgo".to_string(),
                asset_path: "/assets/stamps/pressed-ginkgo.png".to_string(),
                x_pct: 80.0,
                y_pct: 14.0,
                rotation_deg: -9.5,
                scale: 1.0,
                label: Some("Pressed Ginkgo".to_string()),
            },
            StampPlacement {
                id: "seed-postmark-1".to_string(),
                asset_id: "postmark-circle".to_string(),
                asset_path: "/assets/svg/postmark-circle.svg".to_string(),
                x_pct: 66.0,
                y_pct: 76.0,
                rotation_deg: 6.0,
                scale: 1.05,
                label: Some("SANCTUARY POST".to_string()),
            },
        ];

        let body_1 = "\
By half past nine the sky over the courtyard thinned into pale pewter, and a steady curtain of rain began tapping the teak window sill. The brass lamp casts just enough amber across the desk to keep the morning unhurried.

I opened the linen notebook before checking any messages. There is a different cadence to thoughts written on paper—fewer sharp turns, more room to notice the smell of wet limestone and the cardamom steam rising from a chipped porcelain cup.

> *Keep the quiet hours uncrowded; everything worth remembering arrives slowly when the desk is clear.*

This afternoon's rain echoes the notes I kept in [[First Rain of November]], when the season first shifted and the fern fronds along the east wall uncurled overnight.

- [x] Steep ginger-cardamom tea before opening the ledger
- [x] Sketch the veranda light while the cloud ceiling hangs low
- [ ] Re-read last year's hillside notes before evening";

        vault.save_entry_with_frontmatter(fm1, body_1)?;
        entries_seeded += 1;
    }

    // -----------------------------------------------------------------------
    // Entry 2: "First Rain of November" (bidirectional link back to Entry 1)
    // -----------------------------------------------------------------------
    let title_2 = "First Rain of November";
    if !has_title(title_2) {
        let dt2 = Utc
            .from_utc_datetime(&nov_first.and_hms_opt(17, 45, 0).expect("valid time"));
        let mut fm2 = EntryFrontmatter::new(title_2);
        fm2.id = Uuid::now_v7();
        fm2.date = dt2;
        fm2.updated_at = dt2;
        fm2.notebook_id = Some("daily".to_string());
        fm2.tags = vec!["november".to_string(), "monsoon".to_string(), "tea".to_string()];
        fm2.mood = Some("calm".to_string());
        fm2.weather = Some(WeatherInfo {
            condition: "Showers".to_string(),
            temperature_c: Some(25.2),
            icon_key: "rain".to_string(),
            humidity_pct: Some(81),
            summary: Some("Cool evening gusts smelling of rain-soaked earth".to_string()),
        });
        fm2.location = Some(colombo_location());
        fm2.stamps = vec![StampPlacement {
            id: "seed-washi-2".to_string(),
            asset_id: "watercolor-washi-sage".to_string(),
            asset_path: "/assets/stamps/watercolor-washi-sage.png".to_string(),
            x_pct: 78.0,
            y_pct: 18.0,
            rotation_deg: 4.5,
            scale: 0.95,
            label: Some("Sage Washi".to_string()),
        }];

        let body_2 = "\
The air broke at dusk—three days of humid stillness folding all at once into cool wind across the neem leaves. Every roof tile along the lane answered the first heavy drops with a soft clay percussion.

I pulled the wicker chair closer to the doorway and wrote with a broad nib on cream cotton paper, testing a bottle of warm walnut ink. Even a short page changes how an evening settles; weeks later these lines still lead straight into [[Rain on the Teak Sill & Unhurried Ink]].

### Evening Shelf Notes
- Finished the last chapters of the botanical travelogue
- Set aside two cedar bookmarks for the winter notebooks";

        vault.save_entry_with_frontmatter(fm2, body_2)?;
        entries_seeded += 1;
    }

    // -----------------------------------------------------------------------
    // Entry 3 (Exactly 1 Year Ago): "Autumn Tea in Kandy — One Year Ago"
    // -----------------------------------------------------------------------
    let title_3 = "Autumn Tea in Kandy — One Year Ago";
    if !has_title(title_3) {
        let dt3 = Utc
            .from_utc_datetime(&one_year_ago.and_hms_opt(16, 10, 0).expect("valid time"));

        let kandy_photo = load_or_generate_kandy_photo(vault)?;

        let mut fm3 = EntryFrontmatter::new(title_3);
        fm3.id = Uuid::now_v7();
        fm3.date = dt3;
        fm3.updated_at = dt3;
        fm3.notebook_id = Some("daily".to_string());
        fm3.tags = vec![
            "kandy".to_string(),
            "travel".to_string(),
            "tea".to_string(),
            "archive".to_string(),
        ];
        fm3.mood = Some("grateful".to_string());
        fm3.starred = true;
        fm3.weather = Some(WeatherInfo {
            condition: "Hillside Mist".to_string(),
            temperature_c: Some(21.5),
            icon_key: "fog".to_string(),
            humidity_pct: Some(88),
            summary: Some("Low cloud ribbons drifting above the lake".to_string()),
        });
        fm3.location = Some(GeoLocation {
            place_name: "Kandy Timber Pavilion".to_string(),
            city: Some("Kandy".to_string()),
            country: Some("Sri Lanka".to_string()),
            latitude: Some(7.2906),
            longitude: Some(80.6337),
        });
        fm3.photos = vec![kandy_photo];
        fm3.stamps = vec![
            StampPlacement {
                id: "seed-airmail-3".to_string(),
                asset_id: "vintage-airmail-flora".to_string(),
                asset_path: "/assets/stamps/vintage-airmail-flora.png".to_string(),
                x_pct: 81.0,
                y_pct: 16.0,
                rotation_deg: 7.5,
                scale: 1.0,
                label: Some("Airmail Flora".to_string()),
            },
            StampPlacement {
                id: "seed-postmark-3".to_string(),
                asset_id: "postmark-botanical".to_string(),
                asset_path: "/assets/svg/postmark-botanical.svg".to_string(),
                x_pct: 64.0,
                y_pct: 78.0,
                rotation_deg: -5.0,
                scale: 1.0,
                label: Some("Botanical Archive".to_string()),
            },
        ];

        let body_3 = "\
A year ago today the train climbed out of the lowlands into cedar mist and woodsmoke. At the timber pavilion above Kandy Lake, an elderly tea steward poured amber broken-orange-pekoe into thick stoneware bowls while rain silvered the botanical gardens below.

We spoke very little for half an hour. Outside, silver-oak leaves turned slowly in the updraft, and someone down the ridge practiced a slow bamboo flute scale until the streetlights flickered on.

> *How rarely we notice at the time which ordinary afternoons will become anchor stones in memory.*";

        vault.save_entry_with_frontmatter(fm3, body_3)?;
        entries_seeded += 1;
    }

    // -----------------------------------------------------------------------
    // Entry 4 (Second Notebook): "Pressed Pteris Fern & Herbarium Notes"
    // -----------------------------------------------------------------------
    let title_4 = "Pressed Pteris Fern & Herbarium Notes";
    if !has_title(title_4) {
        let dt4 = Utc
            .from_utc_datetime(&herbarium_day.and_hms_opt(11, 5, 0).expect("valid time"));
        let mut fm4 = EntryFrontmatter::new(title_4);
        fm4.id = Uuid::now_v7();
        fm4.date = dt4;
        fm4.updated_at = dt4;
        fm4.notebook_id = Some(botanical_nb_id);
        fm4.tags = vec![
            "botany".to_string(),
            "herbarium".to_string(),
            "field-notes".to_string(),
        ];
        fm4.mood = Some("curious".to_string());
        fm4.weather = Some(WeatherInfo {
            condition: "Partly Cloudy".to_string(),
            temperature_c: Some(26.4),
            icon_key: "partly-cloudy".to_string(),
            humidity_pct: Some(74),
            summary: Some("Filtered morning sun through the canopy".to_string()),
        });
        fm4.location = Some(GeoLocation {
            place_name: "Viharamahadevi Walk".to_string(),
            city: Some("Colombo".to_string()),
            country: Some("Sri Lanka".to_string()),
            latitude: Some(6.9135),
            longitude: Some(79.8617),
        });
        fm4.stamps = vec![StampPlacement {
            id: "seed-pen-4".to_string(),
            asset_id: "botanical-fountain-pen".to_string(),
            asset_path: "/assets/stamps/botanical-fountain-pen.png".to_string(),
            x_pct: 79.0,
            y_pct: 15.0,
            rotation_deg: -6.0,
            scale: 1.0,
            label: Some("Fountain Pen".to_string()),
        }];

        let body_4 = "\
Collected two fallen fronds of *Pteris vittata* near the shaded masonry wall after yesterday's shower. Pressed between sheets of blotting paper beneath the heavy dictionary for six days, their pinnae have dried to a quiet olive-sage without curling at the margins.

### Specimen Ledger
- **Specimen**: Ladder brake fern (*Pteris vittata*)
- **Ink Mix**: 3 parts walnut sepia to 1 part mossy green
- **Observation**: Spore bands trace a clean rust-gold seam along the underside of the mature leaflets

Paired the pressing with a quick study of [[Rain on the Teak Sill & Unhurried Ink]] so both notebooks stay braided together.";

        vault.save_entry_with_frontmatter(fm4, body_4)?;
        entries_seeded += 1;
    }

    Ok(SeedSummary {
        vault_path: vault.root().display().to_string(),
        notebooks_created,
        entries_seeded,
    })
}

fn colombo_location() -> GeoLocation {
    GeoLocation {
        place_name: "Colombo Veranda".to_string(),
        city: Some("Colombo".to_string()),
        country: Some("Sri Lanka".to_string()),
        latitude: Some(6.9271),
        longitude: Some(79.8612),
    }
}

fn load_or_generate_kandy_photo(vault: &Vault) -> Result<PolaroidMeta> {
    let bytes = if let Some(cover_path) = resolve_repo_path("assets/covers/botanical-fern-wash.png")
    {
        std::fs::read(cover_path)?
    } else {
        generate_veranda_polaroid_png()?
    };

    let mut photo = vault.ingest_photo(
        "kandy-tea-mist.png",
        &bytes,
        Some("Afternoon tea & botanical wash in Kandy".to_string()),
    )?;
    photo.rotation_deg = 2.1;
    photo.washi_asset = Some("/assets/svg/washi-cream.svg".to_string());
    photo.location = Some(GeoLocation {
        place_name: "Kandy Timber Pavilion".to_string(),
        city: Some("Kandy".to_string()),
        country: Some("Sri Lanka".to_string()),
        latitude: Some(7.2906),
        longitude: Some(80.6337),
    });
    Ok(photo)
}
