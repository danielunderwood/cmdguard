# METADATA
# entrypoint: true
package cmdguard

import rego.v1

# ==========================================================================
# Declarative subcommand allowlists
# ==========================================================================
# These tables automatically generate allow rules via stdlib.
# Use input.binary_name (resolved binary) for matching.

allowed_subcommands["git"] := {
	"status", "diff", "log", "branch", "show",
	"fetch", "stash", "remote", "tag", "describe",
	"add", "commit", "restore", "switch", "checkout",
}

allowed_subcommands["cargo"] := {
	"build", "test", "check", "fmt", "clippy", "run", "doc",
}

# ==========================================================================
# Exclusion tables — narrow the base allowlists
# ==========================================================================
# Use denied_subcommands to carve out exceptions from allowed_subcommands.
# For example, to block `cargo publish` even if it were in the allow table:
#   denied_subcommands["cargo"] := {"publish"}

# ==========================================================================
# Explicit rules using parsed_flags
# ==========================================================================

is_force_push if input.parsed_flags.force == true

is_force_push if input.parsed_flags.force_with_lease == true

# Allow git push without --force / --force-with-lease
rules["allow_git_push"] := allow("git push") if {
	input.binary_name == "git"
	input.subcommand == "push"
	not is_force_push
}

# Force pushes are destructive but sometimes intentional - ask the user.
# Prefer --force-with-lease, which won't overwrite other people's commits.
rules["ask_force_push"] := ask("Force push - prefer --force-with-lease over --force") if {
	input.binary_name == "git"
	input.subcommand == "push"
	input.parsed_flags.force == true
	not input.parsed_flags.force_with_lease == true
}

rules["ask_force_with_lease_push"] := ask("Force-with-lease push - confirm overwrite") if {
	input.binary_name == "git"
	input.subcommand == "push"
	input.parsed_flags.force_with_lease == true
}

# Ask for git reset --hard (destructive but sometimes needed)
rules["ask_git_reset_hard"] := ask("git reset --hard discards uncommitted changes") if {
	input.binary_name == "git"
	input.subcommand == "reset"
	input.parsed_flags.hard == true
}

# ==========================================================================
# Safe commands pattern — simple binary allowlists
# ==========================================================================

# Package manager commands
rules["safe_package_manager"] := allow("Safe package manager operation") if {
	input.binary_name in {"npm", "yarn", "pnpm"}
	input.subcommand in {"install", "run", "test", "build", "start", "dev"}
}

# Read-only commands
rules["safe_readonly"] := allow("Read-only command") if {
	input.command[0] in {"ls", "cat", "head", "tail", "grep", "find", "wc", "file", "which"}
}

# Echo commands
rules["safe_echo"] := allow("Echo command") if {
	input.command[0] in {"echo", "printf"}
}

# ==========================================================================
# Deny rules — block dangerous operations
# ==========================================================================

# Deny dangerous commands
rules["deny_dangerous"] := deny("This command is blocked for safety") if {
	input.command[0] in {"shutdown", "reboot", "mkfs", "dd"}
}

# Deny recursive delete outside project
rules["deny_rm_outside_project"] := deny("Recursive delete outside project root is blocked") if {
	input.binary_name == "rm"
	input.parsed_flags.recursive
	path_outside_project
}
