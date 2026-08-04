use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "cmdguard")]
#[command(about = "Policy-driven permission control for AI coding agents")]
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

    /// Check policy configuration for sharp edges
    Lint {
        /// Policy directory (default: ~/.config/cmdguard)
        #[arg(short, long)]
        policy_dir: Option<PathBuf>,

        /// Minimum severity that should make lint exit non-zero
        #[arg(long, value_enum, default_value_t = LintFailOn::Error)]
        fail_on: LintFailOn,
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

    /// Manage coding-agent hook registration
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LintFailOn {
    Error,
    Warning,
}

#[derive(Subcommand)]
pub enum HookAction {
    /// Register cmdguard in the selected agent's hook configuration
    Install {
        /// Agent hook protocol to install
        #[arg(long, value_enum, default_value_t = HookTarget::Claude)]
        target: HookTarget,
    },
    /// Remove cmdguard from hooks
    Uninstall {
        /// Agent hook protocol to uninstall
        #[arg(long, value_enum, default_value_t = HookTarget::Claude)]
        target: HookTarget,
    },
    /// Show hook registration status
    Status {
        /// Agent hook protocol to inspect (omit to show all targets)
        #[arg(long, value_enum)]
        target: Option<HookTarget>,
    },
    /// Read a hook payload from stdin and emit a permission decision.
    Run {
        /// Policy directory (default: ~/.config/cmdguard)
        #[arg(short, long)]
        policy_dir: Option<PathBuf>,

        /// Agent hook protocol that supplied the payload
        #[arg(long, value_enum, default_value_t = HookTarget::Claude)]
        target: HookTarget,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HookTarget {
    Claude,
    Codex,
}

#[derive(Subcommand)]
pub enum BaseAction {
    /// Write embedded base policies to ~/.config/cmdguard/base/
    Sync,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_status_without_target_requests_overview() {
        let cli = Cli::try_parse_from(["cmdguard", "hook", "status"]).unwrap();
        match cli.command {
            Some(Commands::Hook {
                action: HookAction::Status { target },
            }) => assert_eq!(target, None),
            _ => panic!("expected hook status command"),
        }
    }

    #[test]
    fn hook_status_accepts_specific_target() {
        let cli = Cli::try_parse_from(["cmdguard", "hook", "status", "--target", "codex"]).unwrap();
        match cli.command {
            Some(Commands::Hook {
                action: HookAction::Status { target },
            }) => assert_eq!(target, Some(HookTarget::Codex)),
            _ => panic!("expected hook status command"),
        }
    }
}
