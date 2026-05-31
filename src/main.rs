use clap::{Parser, Subcommand};

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
    },
    /// Compare two or more sessions side-by-side
    Compare {
        /// Paths to session JSONL files
        paths: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Analyze { path, json, csv } => {
            eprintln!("TODO: analyze {}", path);
        }
        Commands::Compare { paths, json } => {
            eprintln!("TODO: compare {} sessions", paths.len());
        }
    }
}
