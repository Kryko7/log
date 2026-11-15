//! Botanical die-cut stickers and rotary postal ink stamp overlays + picker popover.

use journal_types::{AssetManifest, StampPlacement};
use leptos::prelude::*;

use crate::icons::{IconClose, IconPlus, IconStamp};

fn is_postmark_asset(path: &str) -> bool {
    path.ends_with(".svg") || path.contains("postmark")
}

/// Overlay layer rendering [`StampPlacement`]s across a journal folio.
///
/// Supports drag repositioning, rotation (`-20°..+20°`), scale adjustments, and removal when editable.
#[component]
pub fn StampStickerLayer(
    stamps: Signal<Vec<StampPlacement>>,
    #[prop(optional)] editable: Option<bool>,
    #[prop(optional)] on_change: Option<Callback<Vec<StampPlacement>>>,
    #[prop(optional)] on_update: Option<Callback<Vec<StampPlacement>>>,
    #[prop(optional)] on_remove: Option<Callback<String>>,
) -> impl IntoView {
    let layer_ref = NodeRef::<leptos::html::Div>::new();
    let selected_id = RwSignal::new(Option::<String>::None);
    let dragging_id = RwSignal::new(Option::<String>::None);

    let update_cb = on_change.or(on_update);
    let is_editable = editable.unwrap_or(update_cb.is_some() || on_remove.is_some());

    let on_mousemove = move |ev: web_sys::MouseEvent| {
        if !is_editable {
            return;
        }
        let Some(active_id) = dragging_id.get_untracked() else {
            return;
        };
        let Some(layer_el) = layer_ref.get() else {
            return;
        };
        let rect = layer_el.get_bounding_client_rect();
        let w = rect.width().max(1.0);
        let h = rect.height().max(1.0);
        let x_pct = (((ev.client_x() as f64 - rect.left()) / w) * 100.0).clamp(6.0, 94.0) as f32;
        let y_pct = (((ev.client_y() as f64 - rect.top()) / h) * 100.0).clamp(6.0, 94.0) as f32;

        if let Some(cb) = update_cb {
            let mut next = stamps.get_untracked();
            if let Some(item) = next.iter_mut().find(|s| s.id == active_id) {
                item.x_pct = x_pct;
                item.y_pct = y_pct;
            }
            cb.run(next);
        }
    };

    let stop_drag = move |_| {
        if dragging_id.get_untracked().is_some() {
            dragging_id.set(None);
        }
    };

    view! {
        <div
            node_ref=layer_ref
            class="stamp-sticker-layer"
            class:is-editable=is_editable
            class:is-dragging=move || dragging_id.get().is_some()
            on:mousemove=on_mousemove
            on:mouseup=stop_drag
            on:mouseleave=stop_drag
            on:click=move |_| {
                if is_editable {
                    selected_id.set(None);
                }
            }
        >
            {move || {
                stamps
                    .get()
                    .into_iter()
                    .map(|stamp| {
                        let id_stored = StoredValue::new(stamp.id.clone());
                        let postmark = is_postmark_asset(&stamp.asset_path);
                        let is_selected = move || {
                            is_editable
                                && selected_id.get().as_deref() == Some(id_stored.get_value().as_str())
                        };

                        view! {
                            <div
                                class="placed-stamp-node"
                                class:is-postmark=postmark
                                class:is-diecut=!postmark
                                class:is-selected=is_selected
                                style=format!(
                                    "left: {:.2}%; top: {:.2}%; transform: translate(-50%, -50%) rotate({:.1}deg) scale({:.2});",
                                    stamp.x_pct, stamp.y_pct, stamp.rotation_deg, stamp.scale
                                )
                                on:mousedown=move |ev| {
                                    if !is_editable {
                                        return;
                                    }
                                    ev.stop_propagation();
                                    let id_val = id_stored.get_value();
                                    selected_id.set(Some(id_val.clone()));
                                    dragging_id.set(Some(id_val));
                                }
                                on:click=move |ev| {
                                    if is_editable {
                                        ev.stop_propagation();
                                    }
                                }
                            >
                                <img
                                    src=stamp.asset_path.clone()
                                    alt=stamp.label.clone().unwrap_or_else(|| stamp.asset_id.clone())
                                    class="placed-stamp-img"
                                    draggable="false"
                                />
                                {stamp.label.clone().filter(|l| !l.trim().is_empty() && postmark).map(|lbl| {
                                    view! { <span class="placed-stamp-label">{lbl}</span> }
                                })}

                                <Show when=is_selected>
                                    <div
                                        class="stamp-mini-controls"
                                        on:mousedown=move |ev| ev.stop_propagation()
                                        on:click=move |ev| ev.stop_propagation()
                                    >
                                        <button
                                            type="button"
                                            class="stamp-ctrl-btn"
                                            title="Rotate counter-clockwise"
                                            on:click=move |_| {
                                                if let Some(cb) = update_cb {
                                                    let target = id_stored.get_value();
                                                    let mut next = stamps.get_untracked();
                                                    if let Some(s) = next.iter_mut().find(|s| s.id == target) {
                                                        s.rotation_deg = (s.rotation_deg - 5.0).clamp(-20.0, 20.0);
                                                    }
                                                    cb.run(next);
                                                }
                                            }
                                        >
                                            "-5°"
                                        </button>
                                        <button
                                            type="button"
                                            class="stamp-ctrl-btn"
                                            title="Rotate clockwise"
                                            on:click=move |_| {
                                                if let Some(cb) = update_cb {
                                                    let target = id_stored.get_value();
                                                    let mut next = stamps.get_untracked();
                                                    if let Some(s) = next.iter_mut().find(|s| s.id == target) {
                                                        s.rotation_deg = (s.rotation_deg + 5.0).clamp(-20.0, 20.0);
                                                    }
                                                    cb.run(next);
                                                }
                                            }
                                        >
                                            "+5°"
                                        </button>
                                        <button
                                            type="button"
                                            class="stamp-ctrl-btn"
                                            title="Decrease size"
                                            on:click=move |_| {
                                                if let Some(cb) = update_cb {
                                                    let target = id_stored.get_value();
                                                    let mut next = stamps.get_untracked();
                                                    if let Some(s) = next.iter_mut().find(|s| s.id == target) {
                                                        s.scale = (s.scale - 0.1).clamp(0.6, 1.6);
                                                    }
                                                    cb.run(next);
                                                }
                                            }
                                        >
                                            "−"
                                        </button>
                                        <button
                                            type="button"
                                            class="stamp-ctrl-btn"
                                            title="Increase size"
                                            on:click=move |_| {
                                                if let Some(cb) = update_cb {
                                                    let target = id_stored.get_value();
                                                    let mut next = stamps.get_untracked();
                                                    if let Some(s) = next.iter_mut().find(|s| s.id == target) {
                                                        s.scale = (s.scale + 0.1).clamp(0.6, 1.6);
                                                    }
                                                    cb.run(next);
                                                }
                                            }
                                        >
                                            <IconPlus class="tiny-icon" />
                                        </button>
                                        <button
                                            type="button"
                                            class="stamp-ctrl-btn danger"
                                            title="Remove stamp"
                                            on:click=move |_| {
                                                let del_id = id_stored.get_value();
                                                if let Some(rem) = on_remove {
                                                    rem.run(del_id.clone());
                                                } else if let Some(cb) = update_cb {
                                                    let next: Vec<_> = stamps
                                                        .get_untracked()
                                                        .into_iter()
                                                        .filter(|s| s.id != del_id)
                                                        .collect();
                                                    cb.run(next);
                                                }
                                                selected_id.set(None);
                                            }
                                        >
                                            <IconClose class="tiny-icon" />
                                        </button>
                                    </div>
                                </Show>
                            </div>
                        }
                    })
                    .collect_view()
            }}
        </div>
    }
}

/// Popover palette for picking die-cut botanical stickers and rotary postal stamps.
#[component]
pub fn StampPickerPopover(
    manifest: Signal<AssetManifest>,
    #[prop(optional)] open: Option<Signal<bool>>,
    #[prop(optional)] on_pick: Option<Callback<StampPlacement>>,
    #[prop(optional)] on_select: Option<Callback<StampPlacement>>,
    on_close: Callback<()>,
) -> impl IntoView {
    let rotation = RwSignal::new(-4.0_f32);
    let scale = RwSignal::new(1.0_f32);
    let custom_label = RwSignal::new(String::new());
    let placement_counter = RwSignal::new(1_u32);

    let pick_cb = on_pick.or(on_select);
    let is_open = move || open.map(|s| s.get()).unwrap_or(true);

    let place_asset = move |asset_id: String, asset_path: String, label_default: String| {
        let count = placement_counter.get_untracked();
        placement_counter.set(count.wrapping_add(1));

        let now_ms = js_sys::Date::now() as u64;
        let x_pct = 74.0 + ((count % 3) as f32) * 5.0;
        let y_pct = 18.0 + ((count % 4) as f32) * 14.0;
        let lbl = custom_label.get_untracked().trim().to_string();
        let final_label = if lbl.is_empty() {
            if is_postmark_asset(&asset_path) {
                Some(label_default)
            } else {
                None
            }
        } else {
            Some(lbl)
        };

        let placement = StampPlacement {
            id: format!("st-{now_ms}-{count}"),
            asset_id,
            asset_path,
            x_pct,
            y_pct,
            rotation_deg: rotation.get_untracked().clamp(-20.0, 20.0),
            scale: scale.get_untracked().clamp(0.6, 1.6),
            label: final_label,
        };
        if let Some(cb) = pick_cb {
            cb.run(placement);
        }
        on_close.run(());
    };

    view! {
        <Show when=is_open>
            <div class="stamp-picker-popover" role="dialog" aria-label="Stamps and Stickers">
                <div class="popover-header">
                    <div class="popover-title-row">
                        <IconStamp class="popover-icon" />
                        <span class="small-caps-meta">"Stamps & Stickers"</span>
                    </div>
                    <button
                        type="button"
                        class="icon-btn subtle"
                        title="Close"
                        on:click=move |_| on_close.run(())
                    >
                        <IconClose />
                    </button>
                </div>

                <div class="stamp-tuning-bar">
                    <label class="tuning-field">
                        <span class="small-caps-meta">{move || format!("Tilt {:.0}°", rotation.get())}</span>
                        <input
                            type="range"
                            min="-20"
                            max="20"
                            step="1"
                            prop:value=move || rotation.get().to_string()
                            on:input=move |ev| {
                                if let Ok(v) = event_target_value(&ev).parse::<f32>() {
                                    rotation.set(v.clamp(-20.0, 20.0));
                                }
                            }
                        />
                    </label>
                    <label class="tuning-field">
                        <span class="small-caps-meta">{move || format!("Scale {:.1}×", scale.get())}</span>
                        <input
                            type="range"
                            min="0.6"
                            max="1.5"
                            step="0.1"
                            prop:value=move || format!("{:.1}", scale.get())
                            on:input=move |ev| {
                                if let Ok(v) = event_target_value(&ev).parse::<f32>() {
                                    scale.set(v.clamp(0.6, 1.5));
                                }
                            }
                        />
                    </label>
                </div>

                <input
                    type="text"
                    class="editorial-input stamp-label-input"
                    placeholder="Optional stamp label"
                    prop:value=move || custom_label.get()
                    on:input=move |ev| custom_label.set(event_target_value(&ev))
                />

                <div class="stamp-picker-sections">
                    <div class="stamp-picker-group">
                        <span class="small-caps-meta group-caption">"Stickers"</span>
                        <div class="stamp-grid">
                            {move || {
                                let items = manifest.get().stickers;
                                items
                                    .into_iter()
                                    .map(|item| {
                                        let id = item.id.clone();
                                        let path = item.path.clone();
                                        let label = item.label.clone();
                                        let desc = item.description.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class="stamp-choice-card"
                                                title=desc
                                                on:click=move |_| {
                                                    place_asset(id.clone(), path.clone(), label.clone());
                                                }
                                            >
                                                <img src=item.path.clone() alt=item.label.clone() class="stamp-preview-diecut" />
                                                <span class="stamp-choice-label">{item.label}</span>
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </div>

                    <div class="stamp-picker-group">
                        <span class="small-caps-meta group-caption">"Postmarks"</span>
                        <div class="stamp-grid">
                            {move || {
                                let items = manifest.get().stamps;
                                items
                                    .into_iter()
                                    .map(|item| {
                                        let id = item.id.clone();
                                        let path = item.path.clone();
                                        let label = item.label.clone();
                                        let desc = item.description.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class="stamp-choice-card"
                                                title=desc
                                                on:click=move |_| {
                                                    place_asset(id.clone(), path.clone(), label.clone());
                                                }
                                            >
                                                <img src=item.path.clone() alt=item.label.clone() class="stamp-preview-ink" />
                                                <span class="stamp-choice-label">{item.label}</span>
                                            </button>
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </div>
                </div>
            </div>
        </Show>
    }
}
