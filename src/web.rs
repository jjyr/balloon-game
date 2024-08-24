use crate::Engine;
use crate::{app, setup};
use wasm_bindgen::prelude::wasm_bindgen;

pub fn load_sound_files(eng: &mut Engine) {
    // Skip for now
}

#[wasm_bindgen(start)]
pub async fn run_game() {
    app().run(setup).await.unwrap()
}
