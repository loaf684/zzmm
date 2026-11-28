//! Mod fixer: an offline port of ZZZ-Mod-Fixer
//! (github.com/Vonksdesu/ZZZ-Mod-Fixer, MIT - a continuation of petrascyll's
//! original).
//!
//! Community mods carry the character, IB and texture hashes of the game
//! version they were made for. After a game update those hashes are stale and
//! the mod stops applying. This module rewrites them from a bundled database of
//! every known hash transition, without asking the user to run a Python tool.
//!
//! Layers, top to bottom:
//!
//! - `commands` - the Tauri commands the UI calls.
//! - `ops` - folder walking, encoding, snapshots, apply/undo.
//! - `engine` - the line-based rewrite itself (one INI's text in, text out).
//! - `database` - the generated hash -> fix-command asset.
//! - `undo` - the one bounded pre-fix snapshot per mod, in the config folder.
//! - `store` - the ignore list, in the config folder.
//!
//! Buffer-level fixes (texcoord remaps, blend indices) are **detected and
//! reported, never applied**: they need a pass over `.buf` files that v1 does
//! not implement, and a half-applied one would corrupt the mod. The report
//! marks those files so the UI can say "partly fixed".

pub mod commands;
pub mod database;
pub mod engine;
pub mod model;
pub mod ops;
pub mod store;
pub mod undo;
