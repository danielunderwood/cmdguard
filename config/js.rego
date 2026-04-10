package cmdguard

import rego.v1

allowed_with_args["npm"] := {
	"audit",
	"info",
	"ls",
}

allowed_npm_binaries := {"next", "prettier", "tsc"}

allowed_with_args["npx"] := allowed_npm_binaries
allowed_with_args["pnpx"] := allowed_npm_binaries
