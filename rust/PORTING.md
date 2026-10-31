# C# → Rust porting guide

The C# source is in `../SourceCode` (read-only reference; do not edit it). The Rust workspace is
this folder. Port behaviour **1:1**: same files read/written (relative to the working directory),
same folder names, same console messages (emoji included), same prompts and defaults, same
parallelism level, same skip/overwrite rules. Do not "improve" behaviour; if the C# has an obvious
bug, keep the behaviour and leave a `// NOTE(port):` comment.

## Toolchain

Every shell must load the toolchain first:

```bash
. "E:/Users/simol/AppData/Local/Temp/claude/E--Users-simol-Desktop-all-in-1-converter-win-x64-2-/5398890c-bef7-4e91-a8c5-be47b4f7d3e0/scratchpad/env.sh"
```

Several ports run at once in this workspace, so build **only your own crates** with your **own
target dir**: `CARGO_TARGET_DIR=target-<yourname> cargo test -p <crate>`. Never run a bare
`cargo build` of the whole workspace. Keep your `Cargo.toml` valid at all times (a broken member
manifest breaks every other port). Pure-Rust dependencies only (no C libraries); add new ones to
your crate's `Cargo.toml`, preferring the versions in the workspace `[workspace.dependencies]`.

Do not edit crates you do not own. Do not commit. Do not touch `../SourceCode`.

## Conventions (habbo-core)

| C# | Rust |
|---|---|
| `Console.Write/WriteLine` | `habbo_core::out!` / `outln!` (never `println!`) |
| `Console.ForegroundColor = X` / `ResetColor()` | `console::set_color(Color::X)` / `console::reset_color()`; one line: `outc!(Color::Red, "...")` |
| `Console.ReadLine()` | `console::read_line()` (`""` at EOF) / `read_line_opt()`; `console::prompt("text: ")` |
| `Console.Clear()` | `console::clear()` |
| `{n:N0}` | `console::n0(n)` |
| `IniFileParser.Parse("config.ini")["AppSettings:key"]` | `habbo_core::ini::parse(ini::default_config_path())` or `ini::app_settings()` |
| `ConfigIniFile` | `habbo_core::config::ConfigIniFile` |
| `ConverterSettings.*` | `habbo_core::settings::*` |
| `HabboAssetFolder`, `HofFurniPaths` | `habbo_core::paths::{HabboAssetFolder, hof_furni}`, plus `files_with_ext`, `files_recursive`, `file_stem`, `file_name`, `has_ext` |
| `HttpClient` + `EnsureUserAgent` | `habbo_core::http::{client, get_string, get_bytes, try_get_bytes}` (async reqwest) |
| `FfdecInvocation` | `habbo_core::ffdec::{command, run}` (args as a list, not a string) |
| `JsonReadHelper`, `*IO.LoadAsync/SaveAsync` | `habbo_core::json::{load_object, save_object, *_FILE}` (serde_json, key order kept) |
| `JObject` / `JToken` | `serde_json::Map<String, Value>` / `Value` |
| `AssetWorkspaceRuntime.Router` | `habbo_core::workspace::runtime::router()` |
| `WorkspaceOutput.WriteAllBytesAsync` | `habbo_core::workspace::write_output(path, bytes)` |
| `ConversionSummaryPrinter.PrintSummary` | `habbo_core::summary::print_summary` |
| ImageSharp WebP lossless / PNG | `habbo_core::imaging::{encode_webp_lossless, encode_png, encode_for_extension, decode_rgba}`; pixel work with the `image` crate (`RgbaImage`) |
| `HabBundle.Read/Write/IsHab` | `habbo_nitro::hab::{read, write, is_hab}` |
| `NitroBundle` | `habbo_nitro::bundle::{NitroBundle, read_entries}` |
| `NitroBundler` | `habbo_nitro::bundler::NitroBundler` |
| `AssetBundleWriter` | `habbo_nitro::writer::*` (`extension()`, `set_extension`, `folder`, `exists`, `write`, `build`, `read_files`) |
| `SheetWebp`, `HabSheetConverter.ToWebp` | `habbo_nitro::sheet_webp::{try_convert, to_webp_entry, hab_to_webp}` |
| `BinaryReader` (big-endian) | `habbo_nitro::reader::BinaryReader` |
| `NativeSwfExtractor.Extract / ExtractWithFallbackAsync` | `habbo_swf::{extract, extract_with_fallback}` |

Errors: `anyhow::Result`. Where C# catches and prints, do the same (print, continue). Where C#
lets an exception escape an operation, return `Err`.

## Operations

Every menu operation becomes `pub async fn <name>() -> anyhow::Result<()>` (tokio runtime,
multi-thread). Interactive prompts read with `console::read_line()` exactly like the C# code, so the
GUI can feed answers. Long CPU work: `tokio::task::spawn_blocking` or rayon. Parallel downloads:
`futures::stream::iter(..).buffer_unordered(N)` with the same N as the C# code
(`SemaphoreSlim(N)` / `MaxDegreeOfParallelism`).

At the end, report back the list of public operation functions you created with the C# method each
one replaces (e.g. `habbo_downloads::badges::download_badges` ← `Badges.DownloadBadgesAsync`), so
the operation catalog can be wired.

## Tests

Add unit tests for the pure logic (parsers, mappers, format writers). Use small fixtures built in the
test, not real Habbo files. `cargo test -p <crate>` must pass and `cargo clippy -p <crate>` should be
clean of errors.
