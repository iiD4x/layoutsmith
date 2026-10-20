//! Core of layoutsmith: a lossless model of Android layout XML, a preview
//! layout engine, and design edits (move, resize, align, restyle) expressed as
//! minimal attribute changes.

pub mod edit;
pub mod history;
pub mod layout;
pub mod resources;
pub mod units;
pub mod widgets;
pub mod xml;

pub use history::History;
pub use layout::{ApproxTextMeasurer, LayoutResult, Rect, TextMeasurer, layout};
pub use resources::Resources;
pub use units::{Align, Color, Dim, Gravity};
pub use widgets::{ContainerKind, ViewKind};
pub use xml::{Document, NodeId};
