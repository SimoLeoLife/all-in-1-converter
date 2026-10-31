# All-in-1 Converter — Rust

A Rust port of the C# converter in `../SourceCode`. Same operations, same folders, same
`config.ini`, same output: every download, merge, bundle tool, SWF → `.nitro` / `.hab`
converter, SQL generator and database tool, behind the classic CLI menus and a Professional
window built with [egui](https://github.com/emilk/egui).

## Build and run

```bash
cd rust
cargo build --release
```

The exe is `target/release/habbo-downloader.exe` (Windows) or `target/release/habbo-downloader`
(Linux). Run it from the folder you want to work in: it creates `config.ini` there on first
launch, and the `Habbo_Default/`, `Generate/`, `SWFCompiler/`… folders next to it.

```bash
habbo-downloader                     # asks: Professional window or CLI
habbo-downloader --professional
habbo-downloader --cli               # mainframe-style console menus
habbo-downloader --op tools.swf-furniture-hab   # one operation, no menu
habbo-downloader --benchmark --samples 30
```

Double-clicked, it opens the start window without a console; the console appears only when
you pick the Classic CLI.

`publish-win.bat` / `publish-linux.sh` put the binary and a default `config.ini` in
`../publish/rust-<os>-x64/`.

### Toolchain

- Rust stable (`rustup`). On Windows the GNU toolchain needs MinGW-w64 on `PATH`
  (`scoop install rustup-gnu mingw`); the MSVC toolchain works as well.
- libwebp is compiled from source by the `webp` crate (C compiler required, which MinGW or MSVC
  provides).
- Linux: `pkg-config libssl-dev libxkbcommon-dev libgl1-mesa-dev`.
- Java is optional: only the FFDec fallback for SWFs the built-in reader cannot read needs it.
  FFDec itself (`../Compiled/Tools/ffdec`) is embedded in the exe and unpacked into `Tools/` on
  first launch.

## Crates

| Crate | What it holds | C# source |
|---|---|---|
| `habbo-core` | console routing, `config.ini`, settings, paths, HTTP, FFDec launcher, strict JSON, image encoding, asset workspace, .NET formatting | `Tools/`, `IO/`, `App/Workspaces/` |
| `habbo-swf` | built-in SWF reader, SWF decompiler | `NativeSwfExtractor`, `SwfDecompiler` |
| `habbo-nitro` | `.nitro` / `.hab` bundles, PNG → WebP sheets, bundle tools, Nitro → HAB, size-32 merge | `NitroCompiler/` |
| `habbo-furni` | SWF furniture and pets → `.nitro` / `.hab` | `SWF_Furni_Compiler/`, `SWF_Pets_Compiler/` |
| `habbo-figure` | SWF clothes and effects → `.nitro` / `.hab` | `SWF_Clothes_Compiler/`, `SWF_Effects_Compiler/` |
| `habbo-downloads` | Habbo Original and Nitro custom downloaders | `Download Classes/` |
| `habbo-db` | database tools, SQL generator | `Database/`, `Generate_SQL.cs` |
| `habbo-tools` | furnidata / productdata / clothes merge | `Compare*.cs` |
| `habbo-app` | the exe: operation catalogue, CLI menus, egui window, benchmark | `App/`, `Menu/` |

`PORTING.md` lists the C# → Rust conventions the port follows.

## How it was checked

Each operation was run on the same input by the C# exe and the Rust exe, and the results compared:

| Check | Result |
|---|---|
| SWF → `.hab`: 40 furni, 15 clothes, 36 pets, 252 effects | all identical: json byte-for-byte, sheets pixel-for-pixel |
| furnidata, productdata, texts, variables, effectmap, pets/effects downloads | byte-identical files |
| Generate SQL (65 furni) | byte-identical `.sql`, which also loads into MariaDB |
| 7 database tools on MariaDB 12 | same rows afterwards (offer ids are random in both) and same fix files |
| `--benchmark` | same numbers, 8/8 pixel-perfect |

`cargo test --workspace` runs 150+ unit tests; the shared .NET number formatting is checked
against values printed by .NET 11.

## Differences from the C# build

- Sheets are encoded with libwebp (lossless, method 6, exact): identical pixels, files about
  1 % smaller. `config.ini` `webp_method` (`4` balanced, `1` fast) trades a little size for a
  2–3× faster conversion; every level is lossless.
- A `config.ini` that misses keys takes them from the shipped default instead of failing.
- The Professional window adds:
  - **Cancel** for a running operation, a progress bar fed by its own progress lines, and the
    log in the console's colours;
  - a **queue** ("Add to queue"): runs go one after another, a failure stops the rest;
  - **drag and drop**: `.swf` files are copied into `SWFCompiler/import/<kind>` and converted,
    a `.nitro` / `.hab` opens in the **bundle preview** (sheet + json), also via "Open bundle…";
  - a **run history** kept across sessions (`logs/history.json`, one log file per run);
  - **Test connection** for the database settings;
  - an **update** banner when a newer `rust-v*` release exists (`update_check`, `update_repo`
    in config.ini; also "Check for updates" in both menus).
- The window speaks the languages of the Habbo hotels: English, Italiano, Français, Deutsch,
  Nederlands, Español, Português (Brasil), Türkçe, Suomi (Settings → Language, saved as
  `language` in config.ini; the first start follows the system language). Operation output and
  the CLI stay in English. Texts live in `crates/app/assets/i18n/<code>.json`, keyed by the
  English text; `keys.json` lists them all and a test checks every language has each one.
- **Import new Habbo furniture** (Habbo Original, and `new` in the CLI menu): downloads the current
  furnidata, compares it with `items_base` (or, without a database, with `Habbo_Default/hof_furni`),
  downloads only the new furni and writes their SQL, suggesting the next free id. The database
  itself is not changed.
- Downloads run `download_parallelism` files at a time (default 16, config.ini) and an
  interrupted asset download resumes from its `.part` file; gamedata files always restart.
- Before Fix offer IDs / sit-lay-walk / sprite IDs change anything, the affected tables are saved
  to `Database/Backups/<operation>_<date>.sql` (restore with `mysql … < file`).
- Fix sit / lay / walk no longer fails on a batch without wall items (the C# built invalid SQL).

## Releases

Bump `version` in `rust/Cargo.toml`, then push a matching tag; the *Rust release* workflow builds
both binaries and publishes them, and "Check for updates" finds them:

```bash
git tag rust-v1.1.0 && git push origin rust-v1.1.0
```
- Error texts that came from .NET exceptions read differently where the underlying library
  (reqwest, mysql_async, quick-xml) reports them.
