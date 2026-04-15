package cmdguard

import rego.v1

rules["nickel"] := allow("Allowed nickel command") if {
	input.binary_name == "nickel"
	input.positional.args[0].raw == "eval"
}
