package cmdguard

import rego.v1

allowed_subcommands["cargo"] := {
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

rules["allowed_rustfmt"] := allow("Allowed rust tool") if {
	input.binary_name == "rustfmt"
}

rules["allowed_rustc"] := allow("Allowed rustc command") if {
	input.binary_name == "rustc"
}
