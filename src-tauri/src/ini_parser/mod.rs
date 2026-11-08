//! Lossless INI parsing, hotkey extraction and in-place editing.
//!
//! Every edit rewrites only the characters of the value being changed, so
//! comments, blank lines, ordering and line endings survive a round trip.

pub mod commands;
pub mod hotkeys;
pub mod model;
pub mod parser;
pub mod text;
