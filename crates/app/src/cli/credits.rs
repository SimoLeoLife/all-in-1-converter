//! Credits screen, printed through the console so the CLI and the Professional log both show it.

use habbo_core::console;

const CREDITS: &str = "
================================================================================
                            ALL-IN-1 CONVERTER  -  CREDITS
================================================================================

  AUTHORS
  -------
    Life           -  .NET 11 upgrade, strict JSON-only migration,
                      JSON5 split-layout removal and Professional
                      Avalonia MVVM dashboard.

    medievalshell  -  .NET 10 modernization, cross-platform refactor,
                      ImageSharp migration, original TUI/CLI/GUI shells,
                      JSON5 split-mode layer and Avalonia desktop GUI
                      (Mainframe + Matrix themes).

    duckietm       -  Original all-in-1 downloader / SWF -> Nitro / SQL
                      generator / database tools. Upstream maintainer.

  CONTRIBUTORS
  ------------
    Nitro Team Discord  -  Pet converter base   (discord.gg/yCXcMqrT)
    AtlasOmega          -  Among Us effects (Enable 880-903)
    Leet                -  Enables 500-688

  STACK
  -----
    Rust              |  tokio  |  reqwest  |  serde_json  |  mysql_async
    image + image-webp           (sprite sheets, lossless WebP)
    egui / eframe                (Professional desktop window)
    flate2                       (nitro / hab bundle compression)
    JPEXS Free Flash Decompiler  (Tools/ffdec - SWF extraction fallback)

================================================================================

";

pub fn show() {
    console::write(CREDITS);
}
