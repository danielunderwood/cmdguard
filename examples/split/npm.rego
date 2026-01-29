package claude.permissions

import rego.v1

is_safe_package_manager if {
	input.command[0] in {"npm", "yarn", "pnpm"}
	input.command[1] in {"install", "run", "test", "build", "start", "dev"}
}

decision := "allow" if is_safe_package_manager

reason := "Safe package manager operation" if is_safe_package_manager
