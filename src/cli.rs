use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "cmdguard")]
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

        /// Policy directory (default: ~/.config/cmdguard)
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

    /// Validate Nickel configuration file
    Validate {
        /// Policy directory (default: ~/.config/cmdguard)
        #[arg(short, long)]
        policy_dir: Option<PathBuf>,
    },

    /// Analyze Python code for dangerous patterns (for debugging)
    AnalyzePython {
        /// Python code to analyze
        code: String,
    },

    /// Run a tree-sitter query against code
    Query {
        /// Language to parse (python, bash)
        #[arg(short, long)]
        lang: String,

        /// Inline query string
        #[arg(short, long, conflicts_with = "query_file")]
        query: Option<String>,

        /// Path to query file (.scm)
        #[arg(short = 'f', long, conflicts_with = "query")]
        query_file: Option<PathBuf>,

        /// Code to analyze (or use --file)
        #[arg(conflicts_with = "file")]
        code: Option<String>,

        /// Read code from file instead of argument
        #[arg(long, conflicts_with = "code")]
        file: Option<PathBuf>,
    },

    /// Print version information
    Version,

    /// Manage Claude Code hook registration
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },

    /// Sync embedded base policy files to config directory
    Base {
        #[command(subcommand)]
        action: BaseAction,
    },

    /// Show loaded policies, rules, and tables
    Status {
        /// Policy directory (default: ~/.config/cmdguard)
        #[arg(short, long)]
        policy_dir: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum HookAction {
    /// Register cmdguard as a PreToolUse hook
    Install,
    /// Remove cmdguard from hooks
    Uninstall,
    /// Check if the hook is registered
    Status,
}

#[derive(Subcommand)]
pub enum BaseAction {
    /// Write embedded base policies to ~/.config/cmdguard/base/
    Sync,
}
