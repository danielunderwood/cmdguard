package cmdguard

import rego.v1

rules["safe_package_manager"] := allow("Safe package manager operation") if {
	input.command[0] in {"npm", "yarn", "pnpm"}
	input.command[1] in {"install", "run", "test", "build", "start", "dev"}
}
