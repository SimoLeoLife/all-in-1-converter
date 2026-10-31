# Golden fixtures

Regression tests for the Rust converters: real Habbo SWFs and what the original C# tool
(`Habbo Downloader TEST.exe`) made of them. `cargo test -p habbo-golden`
(`crates/golden/tests/golden.rs`) converts the inputs again with the Rust library functions
and fails on any difference.

## What is here

| Folder | Inputs | Operation |
|---|---|---|
| `furniture/` | 13 furni: animated (`edice`, `rare_dragonlamp`), multi-state teleport (`army_c15_teledoor`), wall items (`window_basic`, `roomdimmer`, `xmas_light`), particles (`fireworks_01`, `antique_c21_wallclock`), 129-sprite score board (`bb_score_b`), queue tile, trophy, bed, static (`duck`) | `tools.swf-furniture-hab` |
| `clothes/` | 6 clothing SWFs (hair, hat, shirt, shoes, trousers, face accessory) | `tools.swf-clothes-hab` |
| `pets/` | 4 pets (`monster`, `fools`, `terrierbaby`, `chicken`) | `tools.swf-pets-hab` |
| `effects/` | 8 effects, two without sprites (`Dance1`, `Ice`) | `tools.swf-effects-hab` |
| `sql/` | `FurnitureData.json` cut down to the sampled furni (and their colour variants) | `tools.generate-sql` (answers `90000`, `777`) |

Each SWF folder holds:

- `swf/` the input files,
- `manifest.json` every bundle the C# tool wrote, with the entry names in their stored order,
- `expected/<name>.json` the json entry exactly as the C# tool wrote it (CRLF line ends, it ran on
  Windows),
- `expected/<name>.png` the sheet, decoded from the C# `.hab`'s WebP and stored as lossless PNG.
  Sheets are compared pixel by pixel, never as encoded bytes (the WebP encoders differ).

`sql/expected.sql` is the C# output (`items_<timestamp>.sql`, renamed). The SQL test builds its
`Generate/default_furniture/*.hab` inputs from `furniture/expected` (the generator only reads the
json), so it needs no extra fixtures. Three sampled furni (`prizetrophy`, `queue_tile1`,
`rare_dragonlamp`) only exist as colour variants in the furnidata, so the "not found in the
FurnitureData" path is covered too.

`.gitattributes` (`* -text`) keeps git from touching line ends; `.gitignore` re-includes `*.swf`,
which the repository ignores elsewhere.

## What is compared

- **Windows**: every bundle entry list, every json byte for byte, every sheet pixel for pixel, the
  SQL text byte for byte (the file name is only checked to be `items_<14 digits>.sql`).
- **Everywhere** (Linux CI too): the same content without depending on the sheet layout: json
  without `spritesheet` (as parsed values), `spritesheet.meta` without `size`, every frame entry
  without `frame.x`/`frame.y`, and every sprite's pixels cropped from each sheet at its own place.
- The console output must not mention FFDec: the native SWF reader has to handle every sample.

Why the OS matters:

- Both tools end json and SQL lines with `Environment.NewLine`. The fixtures hold the Windows
  bytes; on Linux the test expects LF where they have CRLF (`with_platform_newlines`). The bare
  LF inside each SQL INSERT is the same everywhere.
- The sprite packer takes the images in directory listing order, as the C# tool did. NTFS lists
  by name; ext4/tmpfs do not, so on Linux the packing (frame positions, sheet size) differs from
  the Windows output even for the C# tool itself. Hence the layout-independent check there.
- The SQL generator sorts its input files itself, so its text is exact on every OS. Paths only
  appear in console messages, which are not compared.

## Regenerating the expected files

Only when the C# tool's output is meant to change (or to change the sample). For each category,
in a fresh folder with the C# exe's `config.ini` copied in (`download_format=hab`, no workspace):

```bash
mkdir -p work/SWFCompiler/import/furniture && cp config.ini work/
cp tests/golden/furniture/swf/*.swf work/SWFCompiler/import/furniture/
(cd work && echo I | "/path/to/Habbo Downloader TEST.exe" --op tools.swf-furniture-hab)
# clothes / pets / effects: same with import/<category> and --op tools.swf-<category>-hab

cd rust
cargo run -p habbo-golden --bin golden-import -- furniture ../work/SWFCompiler/furniture/hab
```

`golden-import <category> <folder with the .hab files>` rewrites `<category>/expected/` and
`<category>/manifest.json`. Add or remove samples by changing `swf/` before running the C# tool.

SQL: in a fresh folder put `sql/FurnitureData.json` into `Generate/Furnidata/`, the C# furniture
`.hab` files from above into `Generate/default_furniture/`, then

```bash
printf "90000\n777\n" | "/path/to/Habbo Downloader TEST.exe" --op tools.generate-sql
cp Generate/Output_SQL/items_*.sql tests/golden/sql/expected.sql
```

Keep the whole folder small (currently about 1.5 MB); prefer small SWFs.
