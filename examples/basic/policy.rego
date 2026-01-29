# METADATA
# entrypoint: true
package claude.permissions

import rego.v1

import data.claude.permissions.stdlib

default decision := "ask"

is_safe_git_read if {
	input.command[0] == "git"
	stdlib.git_subcommand in {
		"status", "diff", "log", "branch", "show",
		"fetch", "stash", "remote", "tag", "describe",
	}
}

is_safe_git_write if {
	input.command[0] == "git"
	stdlib.git_subcommand in {"add", "commit", "restore", "switch", "checkout"}
}

is_safe_cargo if {
	input.command[0] == "cargo"
	input.command[1] in {"build", "test", "check", "fmt", "clippy", "run", "doc"}
}

is_safe_package_manager if {
	input.command[0] in {"npm", "yarn", "pnpm"}
	input.command[1] in {"install", "run", "test", "build", "start", "dev"}
}

is_read_only_command if {
	input.command[0] in {"ls", "cat", "head", "tail", "grep", "find", "wc", "file", "which"}
}

is_echo_command if {
	input.command[0] in {"echo", "printf"}
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

is_recursive_delete_outside_project if {
	input.command[0] == "rm"
	"-r" in input.flags_expanded
	stdlib.path_outside_project
}

is_dangerous_command if {
	input.command[0] in {"shutdown", "reboot", "mkfs", "dd"}
}

decision := "allow" if is_safe_git_read
decision := "allow" if is_safe_git_write
decision := "allow" if is_safe_cargo
decision := "allow" if is_safe_package_manager
decision := "allow" if is_read_only_command
decision := "allow" if is_echo_command
decision := "deny" if is_force_push
decision := "deny" if is_recursive_delete_outside_project
decision := "deny" if is_dangerous_command

reason := "Safe git read operation" if is_safe_git_read
reason := "Safe git write operation" if is_safe_git_write
reason := "Safe cargo operation" if is_safe_cargo
reason := "Safe package manager operation" if is_safe_package_manager
reason := "Read-only command" if is_read_only_command
reason := "Echo command" if is_echo_command
reason := "Force push is blocked - use regular push instead" if is_force_push
reason := "Recursive delete outside project root is blocked" if is_recursive_delete_outside_project
reason := "This command is blocked for safety" if is_dangerous_command
