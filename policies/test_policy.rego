package claude.permissions

import future.keywords.in

default decision = "ask"

is_safe_git_read {
	input.command[0] == "git"
	data.claude.permissions.stdlib.git_subcommand in {"status", "diff", "log", "branch", "show", "fetch", "stash"}
}

is_force_push {
	input.command[0] == "git"
	data.claude.permissions.stdlib.git_subcommand == "push"
	"--force" in input.command
}

decision = "allow" { is_safe_git_read }
decision = "deny" { is_force_push }

reason = "Safe git read operation" { is_safe_git_read }
reason = "Force push blocked by policy" { is_force_push }
