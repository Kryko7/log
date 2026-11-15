//! Leptos CSR frontend and WebGPU ambient paper/ink shader canvas for **Log**.

pub mod audio_recorder;
pub mod bridge;
pub mod editor;
pub mod gpu_canvas;
pub mod icons;
pub mod sidebar;
pub mod theme;
pub mod views;

use leptos::prelude::*;

/// Root Leptos application component skeleton.
#[component]
pub fn App() -> impl IntoView {
    view! {
        <main class="book-spread-skeleton">
            <p class="small-caps-meta">"Log — Tactile Editorial Sanctuary"</p>
        </main>
    }
}

/// Mount the Leptos application into `#app`.
pub fn mount_app() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
