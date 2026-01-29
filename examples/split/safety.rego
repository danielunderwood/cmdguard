package claude.permissions

import rego.v1

import data.claude.permissions.stdlib

default decision := "ask"

is_read_only_command if {
	input.command[0] in {"ls", "cat", "head", "tail", "grep", "find", "wc", "file", "which"}
}

is_echo_command if {
	input.command[0] in {"echo", "printf"}
}

is_recursive_delete_outside_project if {
	input.command[0] == "rm"
	"-r" in input.flags_expanded
	stdlib.path_outside_project
}

is_dangerous_command if {
	input.command[0] in {"shutdown", "reboot", "mkfs", "dd"}
}

decision := "allow" if is_read_only_command
decision := "allow" if is_echo_command
decision := "deny" if is_recursive_delete_outside_project
decision := "deny" if is_dangerous_command

reason := "Read-only command" if is_read_only_command
reason := "Echo command" if is_echo_command
reason := "Recursive delete outside project root is blocked" if is_recursive_delete_outside_project
reason := "This command is blocked for safety" if is_dangerous_command
