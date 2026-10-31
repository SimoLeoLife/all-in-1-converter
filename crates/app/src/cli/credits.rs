//! Credits screen, printed through the console so the CLI and the Professional log both show it.

use habbo_core::console;

const CREDITS: &str = "
================================================================================
                            ALL-IN-1 CONVERTER  -  CREDITS
================================================================================

  AUTHORS
  -------
    Life           -  Rust port of the whole converter, desktop window
                      (Hotel and Professional looks), catalogue ripper,
                      .NET 11 upgrade and strict JSON-only migration.

    medievalshell  -  .NET 10 modernization, cross-platform refactor,
                      ImageSharp migration, the first CLI / TUI / GUI
                      shells and the GitHub Actions pipeline.

    duckietm       -  Original all-in-1 downloader / SWF -> Nitro / SQL
                      generator / database tools. Upstream maintainer,
                      author of the All-in-1 Installer (the Hotel look).

  CONTRIBUTORS
  ------------
    Nitro Team Discord  -  Pet converter base   (discord.gg/yCXcMqrT)
    AtlasOmega          -  Among Us effects (Enable 880-903)
    Leet                -  Enables 500-688

  BUILT WITH
  ----------
    Rust              |  tokio  |  reqwest  |  serde_json  |  mysql_async
    libwebp                      (sprite sheets, lossless WebP)
    egui / eframe                (desktop window)
    flate2                       (nitro / hab bundle compression)
    JPEXS Free Flash Decompiler  (Tools/ffdec - SWF extraction fallback)
    G-Earth                      (Tools/G-Earth - catalogue ripper)
    Ubuntu, Pixelify Sans        (fonts of the Hotel look)

  Licence GPL-3.0. Everything here is free - don't get scammed.

================================================================================

";

pub fn show() {
    console::write(CREDITS);
}
