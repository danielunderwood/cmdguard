package cmdguard

import rego.v1

in_bin_path(name) if {
	some path in ["", "./target/release/", "./target/debug/"]
	input.command[0] == sprintf("%s%s", [path, name])
}
