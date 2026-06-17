package cmdguard

import rego.v1

is_find if input.command[0] == "find"

is_find_exec_wrapper if {
	"find -exec" in input.wrapper_chain
}

# find uses non-standard single-dash flags (-exec, -delete, -name)
# which the parser doesn't handle via parsed_flags, so we check input.command.
# -exec/-execdir run external commands; -ok/-okdir do the same with confirmation.
exec_like_flags := {"-exec", "-execdir", "-ok", "-okdir"}

find_has_exec if {
	is_find
	some flag in exec_like_flags
	flag in input.command
}

find_has_delete if {
	is_find
	"-delete" in input.command
}

find_exec_has_placeholder if {
	is_find_exec_wrapper
	some arg in input.command
	contains(arg, "{}")
}

find_exec_placeholder_mutator if {
	find_exec_has_placeholder
	input.binary_name in {"touch", "mkdir", "rm", "chmod", "chown", "mv", "cp"}
}

rules["safe_find"] := allow("Allowed find command") if {
	is_find
	not find_has_exec
	not find_has_delete
}

rules["find_with_exec"] := ask("Find with -exec requires approval") if {
	find_has_exec
}

rules["find_with_delete"] := deny("Find with -delete blocked") if {
	find_has_delete
}

rules["find_exec_placeholder_mutation"] := ask("Find -exec mutates placeholder paths - confirm target") if {
	find_exec_placeholder_mutator
}
