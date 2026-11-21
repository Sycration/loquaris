//! The Loquaris project model and the Latin tooling built on collo: word
//! index, search, parsing review and macronizing. Pure Rust without I/O, so it
//! runs on the server and, compiled to wasm, in the browser.
pub use collo;

pub mod language;
pub mod lemma_choice;
pub mod macronize;
pub mod model;
pub mod order;
pub mod review;
pub mod search;
pub mod word_index;

pub use language::{morphology_language, translation_language};
pub use lemma_choice::{OccurrenceCounter, lemma_choice_key};
pub use macronize::{macronize, reader_form};
pub use model::*;
pub use review::{AppliedReview, Review, ReviewModel, ReviewReading, ReviewWord};
pub use search::{
    LessonCount, QueryLemma, QueryReading, QueryWord, SearchMode, SearchOptions, SearchResult,
};
pub use word_index::spaced::{self, ReviewFlag, ReviewKind};
pub use word_index::{
    EntryContent, Hit, LemmaInfo, LessonContent, LessonOutline, ProjectContent, WordIndex,
};

#[cfg(test)]
mod testing;
