package claude.permissions

import rego.v1

# Trust zone fields available in input:
#   input.binary_name - extracted binary name (e.g., "git")
#   input.resolved_path - full path to binary (e.g., "/usr/bin/git")
#   input.resolved_trust_zone - "system", "user", "project", or "unknown"
#   input.is_symlink - true if binary was a symlink
#   input.symlink_source - original path before symlink resolution

rules["safe_command"] := {
	"decision": "allow",
	"reason": "Safe command",
	"priority": 25,
} if {
	input.command[0] in {
		"basename",
		"cat",
		# This could be debated
		"cd",
		"cut",
		"date",
		"df",
		"du",
		"echo",
		"env",
		"file",
		"grep",
		"head",
		"jq",
		"ls",
		"mkdir",
		"printenv",
		"rg",
		"sleep",
		"sort",
		"tail",
		"touch",
		"tr",
		"uniq",
		"wc",
		"which",
	}
}

# Trust zone example: be cautious about unknown binaries
rules["unknown_binary_caution"] := {
	"decision": "ask",
	"reason": "Binary from unknown location - please verify",
	"priority": 45,
} if {
	input.resolved_trust_zone == "unknown"
	# Only apply to potentially dangerous commands
	input.binary_name in {"rm", "chmod", "chown", "mv", "cp"}
}
