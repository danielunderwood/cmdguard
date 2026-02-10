package claude.permissions

import rego.v1

rules["nickel"] := allowed("Allowed nickel command") if {
	input.binary_name == "nickel"
	input.positional.args[0].raw == "eval"
}
