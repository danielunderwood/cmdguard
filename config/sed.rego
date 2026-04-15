package cmdguard

import rego.v1

# Ask for sed -i (in-place edit)
rules["ask_sed_inplace"] := ask("sed -i modifies files in place") if {
	input.binary_name == "sed"
	input.parsed_flags.in_place
}
