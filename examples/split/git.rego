package claude.permissions

import rego.v1

allowed_subcommands["git"] := {
	"status", "diff", "log", "branch", "show",
	"fetch", "stash", "remote", "tag", "describe",
	"ls-tree",
}

rules["safe_git_write"] := allow("Safe git write operation") if {
	input.command[0] == "git"
	input.subcommand in {"add", "commit", "restore", "switch", "checkout"}
}

rules["deny_force_push"] := deny("Force push is blocked - use regular push instead") if {
	input.command[0] == "git"
	input.subcommand == "push"
	some flag in input.command
	flag in {"--force", "-f", "--force-with-lease"}
}
