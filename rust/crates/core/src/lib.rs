//! Shared plumbing for the All-in-1 converter: console output/input routing,
//! config.ini, converter settings, folder layout, HTTP client, FFDec launcher,
//! strict JSON IO, image encoding and the optional Nitro asset workspace.
//!
//! Every operation writes through [`console`] instead of `println!`, so the
//! Professional window can capture the output the same way the C# version
//! redirected `Console.Out`.

pub mod bootstrap;
pub mod config;
pub mod console;
pub mod ffdec;
pub mod hotels;
pub mod http;
pub mod imaging;
pub mod ini;
pub mod json;
pub mod paths;
pub mod settings;
pub mod summary;
pub mod workspace;

pub use console::Color;
