//! All-in-1 Converter: Habbo asset downloader, SWF -> Nitro/HAB converter, SQL generator
//! and database tools. Professional window (egui) or classic console menus.

mod args;
mod benchmark;
mod catalog;
mod cli;
mod embedded_tools;
mod gui;
mod launch_context;
mod ops;
mod runner;

use std::sync::Arc;

use args::{Args, RunMode};
use habbo_core::console::{self, Color};
use habbo_core::outln;

fn main() {
    std::process::exit(real_main());
}

fn real_main() -> i32 {
    // Double-clicked: Windows gave this console exe its own console window. Hide it at once;
    // it comes back only if the Classic CLI is picked. A terminal launch keeps its console.
    let own_console = !launch_context::is_from_terminal();
    if own_console {
        launch_context::set_console_visible(false);
    }

    // Unpack FFDec from the binary on first launch; no-op when it is already on disk.
    embedded_tools::ensure_extracted();
    // The C# build shipped config.ini beside the binary; write it when this folder has none.
    if let Err(error) = habbo_core::ini::ensure_default_config() {
        eprintln!("Could not create config.ini: {error}");
    }
    habbo_core::workspace::runtime::load(&habbo_core::ini::default_config_path());
    console::set_source(Some(Arc::new(cli::terminal::TerminalSource)));

    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut args = Args::parse(&argv);

    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");

    if args.show_help {
        println!("{}", Args::HELP_TEXT);
        return 0;
    }
    if args.show_version {
        let _ = runtime.block_on(ops::display_version());
        return 0;
    }

    // Java is only needed for the FFDec fallback on a SWF the built-in reader cannot read.
    if !habbo_core::bootstrap::is_java_available() {
        console::set_color(Color::Yellow);
        outln!("Java was not found on the command line.");
        outln!("Converting and generating SQL still work - they use the built-in SWF reader.");
        outln!("Only the FFDEC fallback for unusual SWF files is unavailable; install Java");
        outln!("from https://www.java.com/en/download/ if a file ever needs it.");
        console::reset_color();
    }

    // Generate/Furniture became Generate/default_furniture: move an existing folder first.
    let moved = if std::path::Path::new("Generate").is_dir() {
        habbo_db::sql_generator::move_legacy_furniture_folder(std::path::Path::new("Generate"))
    } else {
        Ok(())
    };
    if let Err(error) = moved.map_err(std::io::Error::other).and_then(|_| habbo_core::bootstrap::create_directories()) {
        console::set_color(Color::Red);
        outln!("Error creating directories: {error}");
        console::reset_color();
    }

    // Explorer launch -> the start window; terminal -> the mode prompt.
    let mut show_selector = false;
    if !args.mode_explicitly_set && args.command.is_none() {
        if own_console {
            args.mode = RunMode::Professional;
            show_selector = true;
        } else {
            args.mode = cli::mode_selector::prompt(RunMode::Professional);
            if args.mode == RunMode::Quit {
                outln!("Bye.");
                return 0;
            }
        }
    }

    let mut exit_code = 0;
    let mut desktop_started = false;
    loop {
        {
            let mut host = cli::HOST.lock().unwrap();
            host.switch_requested = false;
            host.mode = args.mode;
        }

        if args.mode == RunMode::Professional {
            // A window can be opened once per process: back from the CLI, start a fresh copy.
            if desktop_started {
                relaunch_professional();
                break;
            }
            desktop_started = true;
            gui::run(runtime.handle().clone(), show_selector);
        } else {
            if own_console {
                launch_context::set_console_visible(true);
            }
            exit_code = match runtime.block_on(cli::run(&args)) {
                Ok(code) => code,
                Err(error) => {
                    eprintln!("{error:#}");
                    1
                }
            };
        }

        let host = cli::HOST.lock().unwrap();
        if !host.switch_requested {
            break;
        }
        args.mode = host.next_mode;
        show_selector = false;
    }
    exit_code
}

fn relaunch_professional() {
    let result = std::env::current_exe().and_then(|exe| {
        let mut command = std::process::Command::new(exe);
        command.arg("--professional").current_dir(std::env::current_dir().unwrap_or_default());
        // The window needs no console: do not open (or share) one for it.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        command.spawn().map(|_| ())
    });
    if let Err(error) = result {
        outln!("Could not reopen the Professional window ({error}). Start the converter again with --professional.");
    }
}
