//! The classic console interface: mainframe-style menus driven by the keyboard.

pub mod credits;
pub mod menus;
pub mod mode_selector;
pub mod presenter;
pub mod terminal;

use std::sync::Mutex;

use anyhow::Result;
use futures::future::BoxFuture;

use crate::args::{Args, RunMode};
use crate::catalog;
use habbo_core::outln;

/// `MenuItem`: one menu entry. `key` is what the operator types.
pub struct MenuItem {
    pub key: String,
    pub label: String,
    pub action: Box<dyn Fn() -> BoxFuture<'static, Result<()>> + Send + Sync>,
    pub how_to_use: Option<String>,
}

impl MenuItem {
    pub fn new(key: &str, label: impl Into<String>, action: impl Fn() -> BoxFuture<'static, Result<()>> + Send + Sync + 'static) -> Self {
        Self { key: key.to_string(), label: label.into(), action: Box::new(action), how_to_use: None }
    }

    pub fn help(mut self, text: &str) -> Self {
        self.how_to_use = Some(text.to_string());
        self
    }

    /// A menu entry that runs a catalogue operation.
    pub fn op(key: &str, label: impl Into<String>, id: &'static str) -> Self {
        Self::new(key, label, move || (catalog::get(id).action)())
    }
}

/// `MenuHost`: the run mode and the "switch UI" request shared by every menu.
pub struct MenuHost {
    pub mode: RunMode,
    pub switch_requested: bool,
    pub next_mode: RunMode,
}

pub static HOST: Mutex<MenuHost> = Mutex::new(MenuHost { mode: RunMode::Cli, switch_requested: false, next_mode: RunMode::Cli });

pub fn request_switch(target: RunMode) {
    let mut host = HOST.lock().unwrap();
    host.next_mode = target;
    host.switch_requested = true;
}

pub fn switch_requested() -> bool {
    HOST.lock().unwrap().switch_requested
}

/// Shows a menu, adding the "?" help entry when any item has a description.
pub async fn show(title: &str, items: Vec<MenuItem>, is_top_level: bool) -> Result<()> {
    let items = with_help(title, items);
    presenter::show(title, items, is_top_level).await
}

fn with_help(title: &str, mut items: Vec<MenuItem>) -> Vec<MenuItem> {
    if items.is_empty() || !items.iter().any(|i| i.how_to_use.as_deref().is_some_and(|h| !h.is_empty())) {
        return items;
    }
    if items.iter().any(|i| i.key.eq_ignore_ascii_case("?")) {
        return items;
    }
    let page = render_help_page(title, &items);
    items.push(MenuItem::new("?", "How to use the entries of this menu", move || {
        let page = page.clone();
        Box::pin(async move {
            habbo_core::console::write(&page);
            Ok(())
        })
    }));
    items
}

fn render_help_page(title: &str, items: &[MenuItem]) -> String {
    let rule = "================================================================================";
    let mut out = format!("\n{rule}\n   HOW TO USE  -  {}\n{rule}\n\n", title.to_uppercase());
    for item in items {
        let Some(help) = item.how_to_use.as_deref().filter(|h| !h.is_empty()) else { continue };
        out.push_str(&format!("  [{}] {}\n", item.key, item.label.to_uppercase()));
        for line in help.split('\n') {
            out.push_str(&format!("      {}\n", line.trim_end()));
        }
        out.push('\n');
    }
    out.push_str(rule);
    out.push_str("\n\n");
    out
}

/// `CliRunner.RunAsync`.
pub async fn run(args: &Args) -> Result<i32> {
    if args.benchmark {
        outln!("The benchmark is not available in this build yet.");
        return Ok(1);
    }

    if let Some(id) = &args.operation {
        let Some(operation) = catalog::find(id) else {
            eprintln!("❌ Error executing operation '{id}': unknown operation id");
            return Ok(1);
        };
        outln!("▶ Executing operation: {} ({})...", operation.title, operation.id);
        if let Err(error) = (operation.action)().await {
            eprintln!("❌ Error executing operation '{id}': {error:#}");
            return Ok(1);
        }
        return Ok(0);
    }

    if let Some(command) = &args.command {
        match command.to_lowercase().as_str() {
            "habbo" => menus::habbo_original().await?,
            "nitro" => menus::nitro_custom().await?,
            "tools" => menus::hotel_tools().await?,
            "database" => menus::database().await?,
            _ => {
                eprintln!("Unknown command: {command}. Valid: habbo, nitro, tools, database.");
                return Ok(2);
            }
        }
        return Ok(0);
    }

    show("Main Menu: Select a Topic", menus::main_menu(), true).await?;
    Ok(0)
}
