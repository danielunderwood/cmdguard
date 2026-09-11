package cmdguard

import rego.v1

# Ask for sed -i (in-place edit)
rules["ask_sed_inplace"] := ask("sed -i modifies files in place") if {
	input.binary_name == "sed"
	input.parsed_flags.in_place
}

# Allow sed when its stdin is a pipe and it isn't editing files in place.
# `cat foo | sed 's/x/y/'` only writes to stdout, so the substitution is
# harmless from the gate's perspective. GNU sed can still execute commands
# via the `e` script flag, so we keep ask_sed_inplace as the in-place
# guard and rely on this rule only for the read-then-transform pattern.
#
# The allowance is only as good as cmdguard's model of the command line: it
# needs every option accounted for (an unmodelled one could be -i spelled a way
# we don't know) and no -f, whose script file can carry `w` and `e` commands.
rules["allow_sed_in_pipe"] := allow("sed in a pipe (stdin -> stdout)") if {
	input.binary_name == "sed"
	input.prev_operator == "|"
	not input.parsed_flags.in_place
	not input.parsed_flags.file
	no_unknown_flags
}
