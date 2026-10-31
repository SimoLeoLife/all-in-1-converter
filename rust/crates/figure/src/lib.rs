//! Clothing and avatar-effect SWF compilers (`SWF_Clothes_Compiler`, `SWF_Effects_Compiler`):
//! export the SWF (native reader, FFDec fallback), map the manifest / animation XML, pack the
//! sprites into one sheet and write the Nitro json + sheet as .nitro or .hab.

pub mod animation;
pub mod assets;
pub mod clothes;
mod driver;
pub mod effects;
pub mod extract;
pub mod spritesheet;
pub mod stj;
pub mod symbols;
pub mod util;
pub mod xml;

pub use clothes::convert_clothes;
pub use effects::convert_effects;
