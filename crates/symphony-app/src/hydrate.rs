use leptos::prelude::*;
use wasm_bindgen::prelude::wasm_bindgen;

#[component]
fn HydrationApp() -> impl IntoView {
    view! { <div></div> }
}

#[wasm_bindgen(start)]
pub fn hydrate() {
    leptos::mount::hydrate_body(HydrationApp);
}
