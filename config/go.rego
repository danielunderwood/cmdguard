package claude.permissions

import rego.v1

allowed_with_args["go"] := {"test", "vet"}

rules["go_mod"] := allow("Allowed go mod") if {
	input.binary_name == "go"
	input.positional.args[0].raw == "mod"
	input.positional.args[1].raw == "tidy"
}
