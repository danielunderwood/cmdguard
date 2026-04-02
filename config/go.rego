package claude.permissions

import rego.v1

allowed_with_args["go"] := {"build", "fmt", "test", "vet", "list", "version"}

rules["gofmt"] := allow("Allowed gofmt") if {
	input.binary_name == "gofmt"
}

rules["golangci-lint"] := allow("Allow golangci-lint") if {
	input.binary_name == "golangci-lint"
}

rules["go_mod"] := allow("Allowed go mod") if {
	input.binary_name == "go"
	input.positional.args[0].raw == "mod"
	input.positional.args[1].raw == "tidy"
}
