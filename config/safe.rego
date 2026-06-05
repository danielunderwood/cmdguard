package cmdguard

import rego.v1

rules["safe_command"] := allow("Safe command") if {
	input.command[0] in {
		"base64",
		"basename",
		"cat",
		"cd",
		"cut",
		"date",
		"df",
		"diff",
		"dig",
		"du",
		"echo",
		"env",
		"file",
		"getent",
		"grep",
		"head",
		"host",
		"jq",
		"ls",
		"lsof",
		"nslookup",
		"paste",
		"ping",
		"printenv",
		"pwd",
		"read",
		"rg",
		"sleep",
		"sort",
		"tail",
		"tr",
		"tree",
		"uniq",
		"wc",
		"which",
		"xxd",
	}
}

rules["unknown_binary_caution"] := ask_at("Binary from unknown location - please verify", 45) if {
	input.resolved_trust_zone == "unknown"
	input.binary_name in {"rm", "chmod", "chown", "mv", "cp"}
}
