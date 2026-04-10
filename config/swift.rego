package cmdguard

import rego.v1

rules["swift_build"] := allow("Allowed swift command") if {
	input.command[0] == "swift"
	input.command[1] == "build"
}
