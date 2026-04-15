package cmdguard

import rego.v1

# Declarative allowlist for read-only git subcommands
allowed_subcommands["git"] := {
	"status", "diff", "log", "branch", "show",
	"fetch", "stash", "remote", "tag", "describe",
	"ls-tree",
}

# Safe git write operations (add, commit, etc.)
rules["safe_git_write"] := allow("Safe git write operation") if {
	input.binary_name == "git"
	input.subcommand in {"add", "commit", "restore", "switch", "checkout"}
}

# Allow push without --force
rules["allow_git_push"] := allow("git push") if {
	input.binary_name == "git"
	input.subcommand == "push"
	not input.parsed_flags.force
}

# Deny force push (parsed_flags-based)
rules["deny_force_push"] := deny("Force push is blocked - use regular push instead") if {
	input.binary_name == "git"
	input.subcommand == "push"
	input.parsed_flags.force == true
}

# Ask for git reset --hard
rules["ask_git_reset_hard"] := ask("git reset --hard discards uncommitted changes") if {
	input.binary_name == "git"
	input.subcommand == "reset"
	input.parsed_flags.hard == true
}
