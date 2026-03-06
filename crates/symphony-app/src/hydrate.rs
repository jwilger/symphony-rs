use wasm_bindgen::prelude::wasm_bindgen;

use crate::ui::hydrate_dashboard;

#[wasm_bindgen]
pub fn hydrate() {
    hydrate_dashboard();
}
