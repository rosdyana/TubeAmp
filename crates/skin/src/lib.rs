//! Classic Winamp 2.x skin (`.wsz`) loading for TubeAmp.
//!
//! Sprite coordinates and the archive/parsing rules are derived from Webamp
//! (MIT, © Jordan Eldredge). See `sprites.rs` and `skin.rs`.

pub mod bitmap;
pub mod parse;
pub mod sheet;
pub mod skin;
pub mod sprites;
#[doc(hidden)]
pub mod testkit;

pub use bitmap::Rgba;
pub use parse::{PlEditColors, Regions};
pub use sheet::Sheet;
pub use skin::{Skin, SkinError, SpriteView};
pub use sprites::SpriteRect;
