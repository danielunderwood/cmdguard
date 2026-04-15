package cmdguard

import rego.v1

# Declarative allowlist for cargo subcommands
allowed_subcommands["cargo"] := {
	"build", "test", "check", "fmt", "clippy", "run", "doc",
}

# Exclusion table — block publish even though it could be allowed
denied_subcommands["cargo"] := {"publish"}
