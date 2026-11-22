//! Full Magic Pack package language, the script interpreter, and the M15 creature renderer.

mod cache;
mod draw;
mod pack;
#[cfg(windows)]
pub mod native;
mod script;

use thiserror::Error;

pub use cache::{extract_pack_zip, pack_zip_url, require_pack, style_path};
pub use draw::{
    average_hash, card_layout, draw_card, fallback_layout, hashes_close, CardLayout, DrawRequest,
    Rect,
};
pub use pack::{Document, Node};
pub use script::{eval_script, Value as ScriptValue};

#[derive(Debug, Error)]
pub enum MseError {
    #[error("{0}")]
    Message(String),
    #[error("missing package: {0}")]
    MissingPackage(String),
    #[error("missing function: {0}")]
    MissingFunction(String),
    #[error("unsupported frame")]
    UnsupportedFrame,
}
