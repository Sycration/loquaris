//! wasm-bindgen interface to loquaris-core for the browser worker. Requests
//! and results are JSON strings (see [`api::Session`] for their shapes);
//! text offsets in results are UTF-16 code units.
pub mod api;

use wasm_bindgen::prelude::*;

/// The word index and the queries the analysis worker answers.
#[wasm_bindgen]
pub struct Analyzer(api::Session);

#[wasm_bindgen]
impl Analyzer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Analyzer {
        Analyzer(api::Session::new())
    }

    #[wasm_bindgen(js_name = setProject)]
    pub fn set_project(&mut self, project: &str) -> Result<(), JsError> {
        self.0.set_project(project).map_err(error)
    }

    #[wasm_bindgen(js_name = setStructure)]
    pub fn set_structure(&mut self, lessons: &str) -> Result<(), JsError> {
        self.0.set_structure(lessons).map_err(error)
    }

    #[wasm_bindgen(js_name = setEntry)]
    pub fn set_entry(&mut self, entry: &str) -> Result<(), JsError> {
        self.0.set_entry(entry).map_err(error)
    }

    #[wasm_bindgen(js_name = removeEntry)]
    pub fn remove_entry(&mut self, id: &str) {
        self.0.remove_entry(id);
    }

    #[wasm_bindgen(js_name = setL1Name)]
    pub fn set_l1_name(&mut self, l1_name: &str) {
        self.0.set_l1_name(l1_name);
    }

    #[wasm_bindgen(js_name = setGaps)]
    pub fn set_gaps(&mut self, gaps: &str) -> Result<(), JsError> {
        self.0.set_gaps(gaps).map_err(error)
    }

    #[wasm_bindgen(js_name = spacedRepetition)]
    pub fn spaced_repetition(&self) -> Result<String, JsError> {
        self.0.spaced_repetition().map_err(error)
    }

    pub fn search(&self, query: &str, options: &str) -> Result<String, JsError> {
        self.0.search(query, options).map_err(error)
    }

    #[wasm_bindgen(js_name = firstUse)]
    pub fn first_use(&self) -> Result<String, JsError> {
        self.0.first_use().map_err(error)
    }

    #[wasm_bindgen(js_name = newInLesson)]
    pub fn new_in_lesson(&self, lesson_id: &str) -> Result<String, JsError> {
        self.0.new_in_lesson(lesson_id).map_err(error)
    }

    pub fn review(&mut self, entry_id: &str) -> Result<String, JsError> {
        self.0.review(entry_id).map_err(error)
    }

    #[wasm_bindgen(js_name = applyReview)]
    pub fn apply_review(
        &mut self,
        entry_id: &str,
        original: &str,
        choices: &str,
    ) -> Result<String, JsError> {
        self.0
            .apply_review(entry_id, original, choices)
            .map_err(error)
    }

    pub fn macronize(&mut self, text: &str) -> String {
        self.0.macronize(text)
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

fn error(message: String) -> JsError {
    JsError::new(&message)
}
