package claude.permissions

import rego.v1

is_npm if input.command[0] == "npm"
is_yarn if input.command[0] == "yarn"
safe_npm_commands := {"build", "test"}

rules["safe_npm"] := allow("Safe npm command") if {
	is_npm
	input.command[1] in safe_npm_commands
}

rules["safe_yarn"] := allow("Safe yarn command") if {
	is_yarn
	input.command[1] in safe_npm_commands
}
