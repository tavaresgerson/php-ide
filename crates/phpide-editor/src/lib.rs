//! The code editor of PHP IDE.
//!
//! - [`selection`]: multi-cursor selections and how they follow edits;
//! - [`movement`] and [`edit`]: caret motions and edit commands, pure
//!   functions from text + selection to a change set + selection;
//! - the text itself, its change log and grouped undo live in
//!   `phpide_core` (`Document`, `Buffer::edit`, `History`).

pub mod decorations;
pub mod edit;
pub mod markdown;
pub mod movement;
pub mod selection;
pub mod style;
pub mod syntax;
pub mod textutil;
pub mod view;

pub use edit::{Edit, EditConfig};
pub use markdown::MarkdownEdit;
pub use movement::{LineVisibility, Motion};
pub use selection::{Range, Selection};
pub use style::{EditorColors, EditorStyle};
pub use syntax::Syntax;
pub use view::{
    ContextTarget, Editor, EditorOptions, EditorOutput, EditorView, GutterClick, MinimapOptions,
    MinimapRender, MinimapSlider, ViewCommand,
};
