//! Command-line flags.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Cli,
    Professional,
    Quit,
}

impl std::fmt::Display for RunMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            RunMode::Cli => "Cli",
            RunMode::Professional => "Professional",
            RunMode::Quit => "Quit",
        })
    }
}

#[derive(Debug, Clone)]
pub struct Args {
    pub mode: RunMode,
    /// True when the user passed --professional/--cli.
    pub mode_explicitly_set: bool,
    pub show_help: bool,
    pub show_version: bool,
    /// Directly opens a top-level menu (CLI only).
    pub command: Option<String>,
    /// Directly runs an operation id.
    pub operation: Option<String>,
    pub benchmark: bool,
    pub benchmark_samples: usize,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            mode: RunMode::Professional,
            mode_explicitly_set: false,
            show_help: false,
            show_version: false,
            command: None,
            operation: None,
            benchmark: false,
            benchmark_samples: 30,
        }
    }
}

impl Args {
    pub fn parse(argv: &[String]) -> Self {
        let mut a = Args::default();
        let mut i = 0;
        while i < argv.len() {
            let arg = argv[i].to_lowercase();
            match arg.as_str() {
                // --gui and --tui are the retired interfaces; they open Professional and CLI.
                "--professional" | "--pro" | "--gui" => {
                    a.mode = RunMode::Professional;
                    a.mode_explicitly_set = true;
                }
                "--cli" | "--tui" => {
                    a.mode = RunMode::Cli;
                    a.mode_explicitly_set = true;
                }
                "--benchmark" | "--bench" => {
                    a.benchmark = true;
                    a.mode = RunMode::Cli;
                    a.mode_explicitly_set = true;
                }
                "--samples" => {
                    if let Some(samples) = argv.get(i + 1).and_then(|s| s.parse().ok()) {
                        a.benchmark_samples = samples;
                        i += 1;
                    }
                }
                "--op" => {
                    if let Some(op) = argv.get(i + 1) {
                        a.operation = Some(op.clone());
                        i += 1;
                    }
                    a.mode = RunMode::Cli;
                    a.mode_explicitly_set = true;
                }
                "--help" | "-h" => a.show_help = true,
                "--version" | "-v" => a.show_version = true,
                "--command" | "-c" => {
                    if let Some(command) = argv.get(i + 1) {
                        a.command = Some(command.clone());
                        i += 1;
                    }
                }
                _ => {
                    if arg.starts_with("--command=") {
                        a.command = Some(argv[i]["--command=".len()..].to_string());
                    } else if arg.starts_with("--op=") {
                        a.operation = Some(argv[i]["--op=".len()..].to_string());
                    } else {
                        eprintln!("Unknown flag: {}", argv[i]);
                    }
                }
            }
            i += 1;
        }
        a
    }

    pub const HELP_TEXT: &'static str = "Habbo Downloader (All-in-1) - usage:\n  \
habbo-downloader [--professional|--cli] [--command <name>] [--op <id>] [--benchmark]\n\
\n\
Modes:\n  \
--professional  Professional dashboard (DEFAULT; Windows + Linux)\n  \
--cli           Classic console menu (keyboard only; for scripts / SSH)\n\
\n\
Options:\n  \
--benchmark        Run WebP lossless compression & pixel fidelity benchmark\n  \
--samples <count>  Number of samples for benchmark (default: 30)\n  \
--op <id>          Directly execute an operation ID (e.g. tools.swf-furniture-hab)\n  \
--command <name>   In CLI mode, jump straight to a top-level menu:\n                     \
habbo | nitro | tools | database\n  \
--version          Print fetched Habbo client version and exit\n  \
--help             Show this help and exit\n";
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        Args::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn flags() {
        let a = parse(&["--CLI", "--op=tools.compile-hab", "-c", "habbo"]);
        assert_eq!(a.mode, RunMode::Cli);
        assert!(a.mode_explicitly_set);
        assert_eq!(a.operation.as_deref(), Some("tools.compile-hab"));
        assert_eq!(a.command.as_deref(), Some("habbo"));

        let b = parse(&["--bench", "--samples", "5"]);
        assert!(b.benchmark);
        assert_eq!(b.benchmark_samples, 5);
        assert!(!parse(&[]).mode_explicitly_set);
    }
}
