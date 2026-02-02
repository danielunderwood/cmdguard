package claude.permissions

import rego.v1

rules["safe_command"] := {
	"decision": "allow",
	"reason": "Safe command",
	"priority": 25,
} if {
	input.command[0] in {
		"basename",
		"cat",
		"cut",
		"df",
		"du",
		"echo",
		"file",
		"grep",
		"head",
		"jq",
		"ls",
		"mkdir",
		"rg",
		"sort",
		"tail",
		"touch",
		"uniq",
		"wc",
		"which",
	}
}
