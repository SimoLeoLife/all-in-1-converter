//! `ConsoleMenuPresenter`: cyan header bar, green double-line frame with the items, red
//! [BACK]/[EXIT], cyan footer; the operator types an item key or "back"/"exit".

use anyhow::Result;
use habbo_core::console::{self, reset_color, set_background, set_color, Color};
use habbo_core::{out, outln};

use super::terminal::frame_width;
use super::MenuItem;

pub async fn show(title: &str, items: Vec<MenuItem>, is_top_level: bool) -> Result<()> {
    let exit_word = if is_top_level { "exit" } else { "back" };

    loop {
        render(title, &items, is_top_level);

        set_background(Color::Black);
        set_color(Color::Green);
        out!("COMMAND:> ");
        let input = console::read_line().trim().to_string();
        reset_color();

        if input.eq_ignore_ascii_case(exit_word) {
            outln!("{}", if is_top_level { "Exiting..." } else { "Returning to the previous menu..." });
            return Ok(());
        }

        let Some(picked) = items.iter().find(|item| item.key.eq_ignore_ascii_case(&input)) else {
            set_color(Color::Red);
            outln!("Unknown command: {input}");
            reset_color();
            outln!("Press any key to continue...");
            console::read_key();
            continue;
        };

        reset_color();
        console::clear();
        print_action_header(title, &picked.label);
        if let Err(error) = (picked.action)().await {
            set_color(Color::Red);
            outln!("Error executing command: {error:#}");
        }
        reset_color();
        outln!();
        if !super::switch_requested() {
            outln!("Press any key to continue...");
            console::read_key();
        }

        // "Switch UI mode" bails out of every nested menu so the outer loop can restart.
        if super::switch_requested() {
            return Ok(());
        }
    }
}

fn header_line(left: &str, center: &str, right: &str, width: usize) -> String {
    let content = left.chars().count() + center.chars().count() + right.chars().count();
    let slack = width.saturating_sub(content + 2).max(2);
    let pad_left = slack / 2;
    let pad_right = slack - pad_left;
    pad_right_to(&format!(" {left}{}{center}{}{right} ", " ".repeat(pad_left), " ".repeat(pad_right)), width)
}

fn pad_right_to(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len >= width { text.to_string() } else { format!("{text}{}", " ".repeat(width - len)) }
}

fn render(title: &str, items: &[MenuItem], is_top_level: bool) {
    reset_color();
    console::clear();
    let w = frame_width();
    let date = format!("DATE: {}", chrono::Utc::now().format("%Y-%m-%d"));

    set_background(Color::Cyan);
    set_color(Color::Black);
    outln!("{}", header_line("ALL-IN-1 CONVERTER", &format!("*** {} ***", title.to_uppercase()), &date, w));
    outln!("{}", "=".repeat(w));
    reset_color();

    set_background(Color::Black);
    set_color(Color::Green);
    outln!("╔{}╗", "═".repeat(w - 2));
    let frame_title = format!(" {} ", title.to_uppercase());
    let space = (w - 2).saturating_sub(frame_title.chars().count());
    outln!("║{}{frame_title}{}║", " ".repeat(space / 2), " ".repeat(space - space / 2));
    outln!("╠{}╣", "═".repeat(w - 2));
    outln!("║{}║", " ".repeat(w - 2));

    for item in items {
        if item.key.eq_ignore_ascii_case("back") {
            continue;
        }
        let mut inner = format!("  [{}] {}", item.key, item.label.to_uppercase());
        if inner.chars().count() > w - 2 {
            inner = inner.chars().take(w - 5).collect::<String>() + "...";
        }
        outln!("║{}║", pad_right_to(&inner, w - 2));
    }
    outln!("║{}║", " ".repeat(w - 2));

    set_color(Color::Red);
    let exit_line = if is_top_level { "  [EXIT] QUIT APPLICATION" } else { "  [BACK] RETURN TO PREVIOUS MENU" };
    outln!("║{}║", pad_right_to(exit_line, w - 2));
    set_color(Color::Green);
    outln!("╚{}╝", "═".repeat(w - 2));

    set_background(Color::Cyan);
    set_color(Color::Black);
    let help = if is_top_level {
        " Type a number to select, or \"exit\" to quit "
    } else {
        " Type a number to select, or \"back\" to return "
    };
    outln!("{}", pad_right_to(help, w));
    reset_color();
}

fn print_action_header(menu_title: &str, action_label: &str) {
    let w = frame_width();
    let date = format!("DATE: {}", chrono::Utc::now().format("%Y-%m-%d"));
    let center = format!("{} -> {}", menu_title.to_uppercase(), action_label.to_uppercase());
    set_background(Color::Cyan);
    set_color(Color::Black);
    outln!("{}", header_line("ALL-IN-1 CONVERTER", &center, &date, w));
    outln!("{}", "=".repeat(w));
    reset_color();
    set_background(Color::Black);
    set_color(Color::Green);
    outln!();
}
