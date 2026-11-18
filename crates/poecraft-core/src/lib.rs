//! poecraft core: Path of Exile 1 crafting simulator (Rust port of the Python reference
//! engine in `poecraft/`). See docs/spec.md.
pub mod analysis;
pub mod builder;
pub mod data;
pub mod engine;
pub mod enumerate;
pub mod exact;
pub mod item;
pub mod itemtext;
pub mod lang;
pub mod linalg;
pub mod mechanics;
pub mod prices;
pub mod rules;
