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

is_force_push if input.parsed_flags.force == true

is_force_push if input.parsed_flags.force_with_lease == true

# Allow push without --force / --force-with-lease
rules["allow_git_push"] := allow("git push") if {
	input.binary_name == "git"
	input.subcommand == "push"
	not is_force_push
}

# Force pushes destructive but sometimes intentional - ask. Prefer
# --force-with-lease, which doesn't blow away other people's work.
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

# Ask for git reset --hard
rules["ask_git_reset_hard"] := ask("git reset --hard discards uncommitted changes") if {
	input.binary_name == "git"
	input.subcommand == "reset"
	input.parsed_flags.hard == true
}
