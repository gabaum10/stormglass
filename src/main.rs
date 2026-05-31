use clap::{Parser, Subcommand};
use std::process;

mod compare;
mod metrics;
mod output;
mod parse;

#[derive(Parser)]
#[command(name = "stormglass")]
#[command(about = "Read the crystals the storm left behind. Claude Code session JSONL analyzer.")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze a Claude Code session transcript
    Analyze {
        /// Path to session JSONL file
        path: String,
        /// Output as JSON instead of human-readable
        #[arg(long)]
        json: bool,
        /// Write per-turn data as CSV to this path
        #[arg(long)]
        csv: Option<String>,
        /// Suppress human-readable summary (use with --csv; --json still prints)
        #[arg(long)]
        quiet: bool,
    },
    /// Compare two or more sessions side-by-side
    Compare {
        /// Paths to session JSONL files
        paths: Vec<String>,
        /// Output as JSON array
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Analyze { path, json, csv, quiet } => {
            // File-not-found: stderr + exit 1
            if !std::path::Path::new(&path).exists() {
                eprintln!("Error: file not found: {}", path);
                process::exit(1);
            }

            let result = match parse::parse_session(&path, csv.as_deref()) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Error reading {}: {}", path, e);
                    process::exit(1);
                }
            };

            // 0 turns: no telemetry, exit 0
            if result.summary.total_turns == 0 {
                println!("0 turns — no telemetry");
                process::exit(0);
            }

            // --json: print JSON summary to stdout
            if json {
                match serde_json::to_string_pretty(&result.summary) {
                    Ok(s) => println!("{}", s),
                    Err(e) => {
                        eprintln!("JSON serialization error: {}", e);
                        process::exit(1);
                    }
                }
            }

            // Human output: unless --quiet or --json (--json is exclusive of human)
            // --csv writes CSV in addition to default human, unless --quiet.
            if !quiet && !json {
                output::print_human(&result.summary, &result.turns);
            }
        }

        Commands::Compare { paths, json } => {
            if paths.is_empty() {
                eprintln!("Error: compare requires at least one session file");
                process::exit(1);
            }

            // Check all files exist before parsing
            for path in &paths {
                if !std::path::Path::new(path).exists() {
                    eprintln!("Error: file not found: {}", path);
                    process::exit(1);
                }
            }

            compare::compare_sessions(&paths, json);
        }
    }
}
