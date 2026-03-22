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
		# YOLO
		"make",
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
		"tree",
		"uniq",
		"wc",
		"which",
		"xcodebuild",
		"xcodegen",
		"xxd",
	}
}

rules["curl_allowed_domains"] := allow("Allow curl") if {
	input.binary_name == "curl"
	regex.match(`^https://(api\.github\.com|raw\.githubusercontent\.com)`, input.positional.url[0].raw)
}

rules["unknown_binary_caution"] := ask_at("Binary from unknown location - please verify", 45) if {
	input.resolved_trust_zone == "unknown"
	input.binary_name in {"rm", "chmod", "chown", "mv", "cp"}
}
