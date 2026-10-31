//! The welcome screen shown in a terminal without --professional/--cli, and the
//! "Switch UI mode" prompt.

use habbo_core::console::{self, reset_color, set_background, set_color, Color};
use habbo_core::{out, outln};

use super::terminal::frame_width;
use crate::args::RunMode;

fn pad(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width { text.to_string() } else { format!("{text}{}", " ".repeat(width - len)) }
}

pub fn prompt(default_mode: RunMode) -> RunMode {
    reset_color();
    console::clear();
    let w = frame_width();

    set_background(Color::Cyan);
    set_color(Color::Black);
    outln!("{}", pad(" ALL-IN-1 CONVERTER  --  HABBO ASSET WORKSTATION ", w));
    outln!("{}", "=".repeat(w));
    reset_color();

    set_background(Color::Black);
    set_color(Color::Green);
    outln!();
    outln!("  Pick how you want to drive the tool today:");
    outln!();
    set_color(Color::Yellow);
    outln!("    [1] PROFESSIONAL - Native dashboard  (recommended)");
    set_color(Color::Green);
    outln!("              Modern cards, native operation pages, live logs and system theme.");
    outln!();
    outln!("    [2] CLI  - Classic keyboard-only console menu");
    outln!();
    outln!("    [Q] QUIT");
    outln!();
    set_color(Color::Yellow);
    let default_name = if default_mode == RunMode::Cli { "CLI" } else { "PROFESSIONAL" };
    outln!("  Tip: Professional is the recommended experience. {default_name} is the default.");
    outln!();

    set_background(Color::Cyan);
    set_color(Color::Black);
    outln!("{}", pad(" Type 1, 2 or Q and press ENTER (empty = default) ", w));
    reset_color();

    set_background(Color::Black);
    set_color(Color::Green);
    out!("CHOICE:> ");
    let raw = console::read_line().trim().to_lowercase();
    reset_color();

    match raw.as_str() {
        "1" | "professional" | "pro" => RunMode::Professional,
        "2" | "cli" => RunMode::Cli,
        "q" | "quit" | "exit" => RunMode::Quit,
        _ => default_mode,
    }
}

/// `UiSwitcher.PromptAndRequestSwitchAsync`.
pub fn prompt_and_request_switch(current: RunMode) {
    let rule = "================================================================================";
    outln!();
    outln!("{rule}");
    outln!("                            SWITCH UI MODE");
    outln!("{rule}");
    outln!();
    outln!("  Current mode: {current}");
    outln!();
    outln!("    [1] PROFESSIONAL - Native dashboard (recommended)");
    outln!("    [2] CLI  - Classic console menu, keyboard only");
    outln!("    [x] CANCEL - stay in the current UI");
    outln!();
    out!("Choice: ");
    let raw = console::read_line().trim().to_lowercase();
    let target = match raw.as_str() {
        "1" | "professional" | "pro" => Some(RunMode::Professional),
        "2" | "cli" => Some(RunMode::Cli),
        _ => None,
    };
    match target {
        Some(target) if target != current => {
            super::request_switch(target);
            outln!("Switching to {target} ... the window/terminal will refresh.");
        }
        _ => outln!("Cancelled - staying in the current UI."),
    }
}
