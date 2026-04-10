package cmdguard

import rego.v1

in_bin_path(name) if {
	some path in ["", "./target/release/", "./target/debug/"]
	input.command[0] == sprintf("%s%s", [path, name])
}

# Deprecated in favor of more general rule
# rules["local_cmdguard"] := {
#     "decision": "allow",
#     "reason": "Allowed local tool",
#     "priority": 25,
# } if in_bin_path("cmdguard")
