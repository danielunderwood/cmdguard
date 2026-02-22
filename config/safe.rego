package claude.permissions

import rego.v1

rules["safe_command"] := allow("Safe command") if {
	input.command[0] in {
		"./scripts/run_postgres_tests.sh",
		"base64",
		"basename",
		"cat",
		"cd",
		"cut",
		"date",
		"df",
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
		"mkdir",
		"nslookup",
		"paste",
		"ping",
		"printenv",
		"pwd",
		"read",
		"rg",
		"sed",
		"sleep",
		"sort",
		"sqq",
		"tail",
		"touch",
		"tr",
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
