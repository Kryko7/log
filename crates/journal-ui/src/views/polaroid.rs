//! Interactive 3D specular-lit Polaroid photograph frame with optional washi tape and lightbox.

use journal_types::PolaroidMeta;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::icons::{IconClose, IconMapPin};

/// Tactile photograph print card with subtle 3D pointer tilt, specular sheen,
/// optional torn washi strip, and serif caption.
#[component]
pub fn PolaroidCard(
    photo: PolaroidMeta,
    #[prop(optional)] index: usize,
    #[prop(optional)] figure_number: Option<usize>,
    #[prop(optional)] editable: Option<bool>,
    #[prop(optional)] on_remove: Option<Callback<String>>,
    #[prop(optional)] on_caption_change: Option<Callback<String>>,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let hovered = RwSignal::new(false);
    let tilt_x = RwSignal::new(0.0_f64);
    let tilt_y = RwSignal::new(0.0_f64);
    let sheen_x = RwSignal::new(50.0_f64);
    let sheen_y = RwSignal::new(30.0_f64);
    let lightbox_open = RwSignal::new(false);

    let is_editable = editable.unwrap_or(on_remove.is_some() || on_caption_change.is_some());
    let base_rot = photo.rotation_deg;
    let img_src = photo
        .thumbnail_url
        .clone()
        .unwrap_or_else(|| photo.url.clone());
    let full_src = photo.url.clone();
    let caption = photo.caption.clone();
    let caption_draft = RwSignal::new(caption.clone().unwrap_or_default());
    let washi = photo.washi_asset.clone();
    let location_label = photo.location.as_ref().map(|loc| loc.place_name.clone());
    let photo_id = photo.id.clone();
    let display_fig = figure_number.unwrap_or(index + 1);
    let figure_label = format!("{:02}", display_fig);
    let alt_text = caption
        .clone()
        .unwrap_or_else(|| photo.filename.clone());

    let on_mousemove = move |ev: web_sys::MouseEvent| {
        let Some(target) = ev.current_target() else {
            return;
        };
        let Ok(el) = target.dyn_into::<web_sys::Element>() else {
            return;
        };
        let rect = el.get_bounding_client_rect();
        let w = rect.width().max(1.0);
        let h = rect.height().max(1.0);
        let rel_x = ((ev.client_x() as f64 - rect.left()) / w).clamp(0.0, 1.0);
        let rel_y = ((ev.client_y() as f64 - rect.top()) / h).clamp(0.0, 1.0);

        let nx = (rel_x - 0.5) * 2.0;
        let ny = (rel_y - 0.5) * 2.0;
        hovered.set(true);
        tilt_x.set(-ny * 5.0);
        tilt_y.set(nx * 5.5);
        sheen_x.set(rel_x * 100.0);
        sheen_y.set(rel_y * 100.0);
    };

    let on_mouseleave = move |_| {
        hovered.set(false);
        tilt_x.set(0.0);
        tilt_y.set(0.0);
        sheen_x.set(50.0);
        sheen_y.set(30.0);
    };

    let wrapper_cls = if class.is_empty() {
        "polaroid-card".to_string()
    } else {
        format!("polaroid-card {class}")
    };

    view! {
        <>
            <figure
                class=wrapper_cls
                class:is-hovered=move || hovered.get()
                style=move || {
                    if hovered.get() {
                        format!(
                            "transform: perspective(1000px) rotateX({:.2}deg) rotateY({:.2}deg) scale3d(1.01, 1.01, 1.01); --sheen-x: {:.1}%; --sheen-y: {:.1}%;",
                            tilt_x.get(),
                            tilt_y.get(),
                            sheen_x.get(),
                            sheen_y.get()
                        )
                    } else {
                        format!(
                            "transform: perspective(1000px) rotate({:.2}deg); --sheen-x: 50%; --sheen-y: 30%;",
                            base_rot
                        )
                    }
                }
                on:mousemove=on_mousemove
                on:mouseleave=on_mouseleave
            >
                {washi.map(|washi_path| {
                    view! {
                        <img
                            src=washi_path
                            alt=""
                            aria-hidden="true"
                            class="polaroid-washi-strip"
                        />
                    }
                })}

                {is_editable
                    .then(|| {
                        on_remove.map(|cb| {
                            let id_rem = photo_id.clone();
                            view! {
                                <button
                                    type="button"
                                    class="polaroid-remove-btn"
                                    title="Remove photograph"
                                    on:click=move |ev| {
                                        ev.stop_propagation();
                                        cb.run(id_rem.clone());
                                    }
                                >
                                    <IconClose />
                                </button>
                            }
                        })
                    })
                    .flatten()}

                <div
                    class="polaroid-frame-window"
                    on:click=move |_| lightbox_open.set(true)
                >
                    <img
                        src=img_src.clone()
                        alt=alt_text.clone()
                        class="polaroid-photo-img"
                        loading="lazy"
                    />
                    <div class="polaroid-specular-sheen" aria-hidden="true"></div>
                </div>

                <figcaption class="polaroid-caption-area">
                    <div class="polaroid-meta-row">
                        <span class="polaroid-fig-num">{figure_label}</span>
                        {location_label.map(|loc| {
                            view! {
                                <span class="polaroid-loc-chip">
                                    <IconMapPin class="tiny-icon" />
                                    <span>{loc}</span>
                                </span>
                            }
                        })}
                    </div>
                    {if let Some(cap_cb) = on_caption_change {
                        view! {
                            <input
                                type="text"
                                class="polaroid-caption-input"
                                placeholder="Add a caption..."
                                prop:value=move || caption_draft.get()
                                on:input=move |ev| caption_draft.set(event_target_value(&ev))
                                on:blur=move |_| cap_cb.run(caption_draft.get_untracked())
                                on:keydown=move |ev| {
                                    if ev.key() == "Enter" {
                                        ev.prevent_default();
                                        cap_cb.run(caption_draft.get_untracked());
                                    }
                                }
                            />
                        }
                            .into_any()
                    } else {
                        caption
                            .clone()
                            .map(|cap| view! { <p class="polaroid-caption-text">{cap}</p> })
                            .into_any()
                    }}
                </figcaption>
            </figure>

            <Show when=move || lightbox_open.get()>
                <div
                    class="editorial-modal-backdrop"
                    on:click=move |_| lightbox_open.set(false)
                >
                    <div
                        class="polaroid-lightbox-sheet"
                        on:click=move |ev| ev.stop_propagation()
                    >
                        <button
                            type="button"
                            class="polaroid-lightbox-close"
                            title="Close photograph"
                            on:click=move |_| lightbox_open.set(false)
                        >
                            <IconClose />
                        </button>
                        <img
                            src=full_src.clone()
                            alt=alt_text.clone()
                            class="polaroid-lightbox-img"
                        />
                        {caption.clone().map(|cap| {
                            view! { <p class="polaroid-lightbox-caption">{cap}</p> }
                        })}
                    </div>
                </div>
            </Show>
        </>
    }
}
