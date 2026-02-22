package claude.permissions

import rego.v1

rules["regal"] := allow("Regal commands allowed") if {
	input.command[0] == "regal"
}
