# METADATA
# entrypoint: true
package cmdguard

import rego.v1

# Declarative subcommand allowlists
allowed_subcommands["git"] := {
	"status", "diff", "log", "branch", "show",
	"fetch", "stash", "remote", "tag", "describe",
}

allowed_subcommands["cargo"] := {
	"build", "test", "check", "fmt", "clippy", "run", "doc",
}

# Safe git write operations
rules["safe_git_write"] := allow("Safe git write operation") if {
	input.command[0] == "git"
	input.subcommand in {"add", "commit", "restore", "switch", "checkout"}
}

# Package manager commands
rules["safe_package_manager"] := allow("Safe package manager operation") if {
	input.command[0] in {"npm", "yarn", "pnpm"}
	input.command[1] in {"install", "run", "test", "build", "start", "dev"}
}

# Read-only commands
rules["safe_readonly"] := allow("Read-only command") if {
	input.command[0] in {"ls", "cat", "head", "tail", "grep", "find", "wc", "file", "which"}
}

# Echo commands
rules["safe_echo"] := allow("Echo command") if {
	input.command[0] in {"echo", "printf"}
}

# Deny force push
rules["deny_force_push"] := deny("Force push is blocked - use regular push instead") if {
	input.command[0] == "git"
	input.subcommand == "push"
	some flag in input.command
	flag in {"--force", "-f", "--force-with-lease"}
}

# Deny dangerous commands
rules["deny_dangerous"] := deny("This command is blocked for safety") if {
	input.command[0] in {"shutdown", "reboot", "mkfs", "dd"}
}

# Deny recursive delete outside project
rules["deny_rm_outside_project"] := deny("Recursive delete outside project root is blocked") if {
	input.command[0] == "rm"
	"-r" in input.flags_expanded
	path_outside_project
}
