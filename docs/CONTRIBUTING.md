# Contributing

## Build and test

```bash
cargo build                    # debug build of everything
cargo run -p habbo-app -- --professional
cargo test --workspace         # unit tests + golden tests (tests/golden)
cargo clippy --workspace --all-targets
```

Windows with the GNU toolchain needs MinGW-w64 on `PATH` (libwebp and the exe resources are
compiled with it). Pure-Rust dependencies are preferred; libwebp is the one C library, for the
sheet compression.

## Output must not change by accident

Hotels load what this tool writes, so the bytes matter. The golden tests in `tests/golden` convert
real Habbo SWFs and compare json byte for byte and sheets pixel for pixel (exact on Windows,
layout-independent on Linux, where directory order differs). If a change is meant to alter output,
regenerate the expected files as described in `tests/golden/README.md` and say why in the commit.

Number and JSON-string formatting follows .NET (`habbo_core::dotnet`), because the formats were
defined by the original C# tool and clients and emulators rely on them.

## Where things go (habbo-core)

| Need | Use |
|---|---|
| Print to the operation output | `habbo_core::out!` / `outln!` (never `println!`, so the window can capture it) |
| Colours | `console::set_color(Color::X)` / `console::reset_color()`; one line: `outc!(Color::Red, "...")` |
| Ask a question | `console::read_line()` (`""` at end of input) / `read_line_opt()`; `console::prompt("text: ")` — the window answers through its input box |
| Thousands separators | `console::n0(n)` |
| config.ini | `habbo_core::ini::parse` (with defaults), `habbo_core::config::ConfigIniFile` (editing), `habbo_core::settings::*` |
| Asset folders | `habbo_core::paths::{HabboAssetFolder, hof_furni}`, `files_with_ext`, `files_recursive`, `file_stem`, `file_name`, `has_ext` |
| HTTP | `habbo_core::http::{client, get_string, get_bytes, try_get_bytes}`; resumable downloads in `habbo_downloads::resume` |
| FFDec | `habbo_core::ffdec::{command, run}` |
| Strict JSON files | `habbo_core::json::{load_object, save_object}` (key order kept) |
| Asset workspace | `habbo_core::workspace::runtime::router()`, `workspace::write_output` |
| End-of-run report | `habbo_core::summary::print_summary` |
| Images | `habbo_core::imaging::{encode_webp_lossless, encode_png, decode_rgba}`; pixels with `image::RgbaImage` |
| .nitro / .hab | `habbo_nitro::{hab, bundle, bundler, writer, sheet_webp}` |
| SWF | `habbo_swf::{extract, extract_with_fallback}` |

Errors: `anyhow::Result`. A failure that should end the operation returns `Err`; one item failing
in a batch is printed and the batch goes on.

## Adding an operation

1. Write `pub async fn <name>() -> anyhow::Result<()>` in the crate it belongs to. Prompts go
   through `console`, CPU-heavy work through `tokio::task::spawn_blocking` or rayon, parallel
   downloads through `futures::stream::iter(..).buffer_unordered(n)`.
2. Add it to `crates/app/src/catalog.rs` (id, category, title, description, asks questions?,
   changes the database?) and to the console menu in `crates/app/src/cli/menus.rs`.
3. Translate its title and description: run the key extraction (the `keys.json` test lists any
   text that is missing) and add the entries to every `crates/app/assets/i18n/<code>.json`.

## Translations

The window's texts are keyed by their English text (`t("Run operation")`, `tf("{count} files", …)`).
`crates/app/assets/i18n/keys.json` lists every key; a test fails when a language misses one or drops a
`{placeholder}`. Use the vocabulary of that language's Habbo community.

## Releases

Bump `version` in `Cargo.toml`, merge to `main`, then push the tag `rust-v<version>`. The Release
workflow tests, builds Windows and Linux binaries and publishes them; running apps offer the update.
