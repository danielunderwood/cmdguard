package cmdguard

import rego.v1

rules["safe_readonly"] := allow("Read-only command") if {
	input.command[0] in {"ls", "cat", "head", "tail", "grep", "find", "wc", "file", "which"}
}

rules["safe_echo"] := allow("Echo command") if {
	input.command[0] in {"echo", "printf"}
}

rules["deny_rm_outside_project"] := deny("Recursive delete outside project root is blocked") if {
	input.command[0] == "rm"
	"-r" in input.flags_expanded
	path_outside_project
}

rules["deny_dangerous"] := deny("This command is blocked for safety") if {
	input.command[0] in {"shutdown", "reboot", "mkfs", "dd"}
}
