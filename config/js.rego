package claude.permissions

import rego.v1

allowed_with_args["npm"] := {
	"audit",
	"info",
	"ls",
}

allowed_with_args["npx"] := {
	"next",
	"tsc",
}
