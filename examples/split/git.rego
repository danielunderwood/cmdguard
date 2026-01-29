# METADATA
# entrypoint: true
package claude.permissions

import rego.v1

import data.claude.permissions.stdlib

is_safe_git_read if {
	input.command[0] == "git"
	stdlib.git_subcommand in {
		"status", "diff", "log", "branch", "show",
		"fetch", "stash", "remote", "tag", "describe",
		"ls-tree",
	}
}

is_safe_git_write if {
	input.command[0] == "git"
	stdlib.git_subcommand in {"add", "commit", "restore", "switch", "checkout"}
}

has_force_flag if {
	some flag in input.command
	flag in {"--force", "-f", "--force-with-lease"}
}

is_force_push if {
	input.command[0] == "git"
	stdlib.git_subcommand == "push"
	has_force_flag
}

decision := "allow" if is_safe_git_read
decision := "allow" if is_safe_git_write
decision := "deny" if is_force_push

reason := "Safe git read operation" if is_safe_git_read
reason := "Safe git write operation" if is_safe_git_write
reason := "Force push is blocked - use regular push instead" if is_force_push
