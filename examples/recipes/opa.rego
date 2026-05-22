package cmdguard

import rego.v1

rules["safe_opa"] := allow("Allowed opa command") if {
	input.command[0] == "opa"
	input.command[1] in {
		"eval",
		"exec",
		"fmt",
		"help",
		"parse",
		"test",
		"version",
	}
}
