package cmdguard

import rego.v1

allowed_subcommands["cargo"] := {
	"build", "test", "check", "fmt", "clippy", "run", "doc",
}
