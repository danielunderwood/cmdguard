package claude.permissions

import rego.v1

allowed_mise_commands := {
	"build",
	"check",
	"t",
	"tasks",
	"test",
	"version",
}

rules["allowed_mise"] := allow("Allowed mise command") if {
	input.command[0] == "mise"
	input.command[1] in allowed_mise_commands
}

# TODO: This will be better with improved flag handling
rules["allowed_mise_env"] := allow("Allowed mise command with env") if {
	regex.match(`[A-Za-z][A-Za-z_]*=.*`, input.command[0])
	input.command[1] == "mise"
	input.command[2] in allowed_mise_commands
}
