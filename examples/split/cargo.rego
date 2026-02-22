package claude.permissions

import rego.v1

allowed_subcommands["cargo"] := {
	"build", "test", "check", "fmt", "clippy", "run", "doc",
}
