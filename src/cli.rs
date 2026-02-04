use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "claude-permissions")]
#[command(about = "Policy-driven permission control for Claude Code")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run policy tests from a YAML file
    Test {
        /// Path to test file (default: looks for policy_tests.yaml in policy dir)
        #[arg(value_name = "FILE")]
        file: Option<PathBuf>,

        /// Show detailed output for each test
        #[arg(short, long)]
        verbose: bool,

        /// Policy directory (default: ~/.config/claude-permissions)
        #[arg(short, long)]
        policy_dir: Option<PathBuf>,
    },

    /// Evaluate a single command (for debugging)
    Eval {
        /// The command to evaluate
        command: String,

        /// Working directory context
        #[arg(short, long, default_value = ".")]
        cwd: String,

        /// Policy directory
        #[arg(short, long)]
        policy_dir: Option<PathBuf>,

        /// Show the JSON input sent to Rego (for policy debugging)
        #[arg(short, long)]
        show_input: bool,
    },

    /// Print version information
    Version,
}
