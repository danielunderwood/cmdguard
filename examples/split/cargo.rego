package claude.permissions

import rego.v1

is_safe_cargo if {
	input.command[0] == "cargo"
	input.command[1] in {"build", "test", "check", "fmt", "clippy", "run", "doc"}
}

decision := "allow" if is_safe_cargo

reason := "Safe cargo operation" if is_safe_cargo
