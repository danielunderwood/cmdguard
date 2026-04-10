package cmdguard

import rego.v1

is_find if input.command[0] == "find"

find_with_exec if {
	is_find
	"-exec" in input.command
}

rules["safe_find"] := allow("Allowed find command") if {
	is_find
	not find_with_exec
}

rules["find_with_exec"] := ask("Find command with -exec requires approval") if find_with_exec
