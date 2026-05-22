package cmdguard

import rego.v1

# Package manager safe subcommands — uses input.binary_name and input.subcommand
rules["safe_package_manager"] := allow("Safe package manager operation") if {
	input.binary_name in {"npm", "yarn", "pnpm"}
	input.subcommand in {"install", "run", "test", "build", "start", "dev"}
}
