package cmdguard

import rego.v1

# Deny rm -r outside project
rules["deny_rm_outside_project"] := deny("Recursive delete outside project blocked") if {
	input.binary_name == "rm"
	input.parsed_flags.recursive
	some target in input.positional.targets
	target.trust_zone != "project"
}

# Deny rm --no-preserve-root
rules["deny_rm_no_preserve_root"] := deny("--no-preserve-root blocked") if {
	input.binary_name == "rm"
	input.parsed_flags.no_preserve_root
}

# Ask for chmod outside project
rules["ask_chmod_outside_project"] := ask("chmod outside project - confirm") if {
	input.binary_name == "chmod"
	some target in input.positional.targets
	target.trust_zone != "project"
}

# Ask for chown outside project
rules["ask_chown_outside_project"] := ask("chown outside project - confirm") if {
	input.binary_name == "chown"
	some target in input.positional.targets
	target.trust_zone != "project"
}
