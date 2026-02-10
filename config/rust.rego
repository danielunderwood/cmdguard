package claude.permissions

import rego.v1

allowed(message) := {
	"decision": "allow",
	"reason": message,
	"priority": 25,
}

rules["allowed_rust_tools"] := allowed("Allowed rust tool") if {
	input.binary_name == "rustfmt"
}

rules["allowed_rustc"] := allowed("Allowed rustc command") if {
	input.binary_name == "rustc"
}

rules["allowed_cargo"] := allowed("Allowed cargo command") if {
	input.binary_name == "cargo"
	input.subcommand in {
		"bench",
		"build",
		"check",
		"clean",
		"clippy",
		"config",
		"doc",
		"fix",
		"fmt",
		"generate-lockfile",
		"help",
		"info",
		"list",
		"locate-project",
		"metadata",
		"pkgid",
		"run",
		"report",
		"rustc",
		"rustdoc",
		"search",
		"test",
		"version",
	}
}
