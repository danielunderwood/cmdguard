package cmdguard

import rego.v1

# Read-only commands
rules["safe_readonly"] := allow("Read-only command") if {
	input.command[0] in {"ls", "cat", "head", "tail", "grep", "find", "wc", "file", "which"}
}

# Echo commands
rules["safe_echo"] := allow("Echo command") if {
	input.command[0] in {"echo", "printf"}
}

# Deny recursive delete outside project (uses parsed_flags)
rules["deny_rm_outside_project"] := deny("Recursive delete outside project root is blocked") if {
	input.binary_name == "rm"
	input.parsed_flags.recursive
	path_outside_project
}

# Deny dangerous commands
rules["deny_dangerous"] := deny("This command is blocked for safety") if {
	input.command[0] in {"shutdown", "reboot", "mkfs", "dd"}
}
