package cmdguard

import rego.v1

rules["ask_shell_output_redirection"] := ask("Shell output redirection writes to file - confirm target") if {
	some redirect in input.redirections
	redirect.writes_to_file
}

path_record_in_project(path) if {
	path.resolved == input.project_root
}

path_record_in_project(path) if {
	startswith(path.resolved, sprintf("%s/", [input.project_root]))
}

command_targets_in_project(values) if {
	count(input.paths) > 0
	every path in input.paths {
		path_record_in_project(path)
	}
}

command_targets_in_project(values) if {
	count(input.paths) == 0
	count(values) > 0
	every path in values {
		path_record_in_project(path)
	}
}

command_has_target_outside_project(values) if {
	some path in input.paths
	not path_record_in_project(path)
}

command_has_target_outside_project(values) if {
	count(input.paths) == 0
	some path in values
	not path_record_in_project(path)
}

rules["allow_touch_in_project"] := allow("touch in project") if {
	input.binary_name == "touch"
	command_targets_in_project(input.positional.files)
}

rules["ask_touch_outside_project"] := ask("touch outside project - confirm") if {
	input.binary_name == "touch"
	command_has_target_outside_project(input.positional.files)
}

rules["allow_mkdir_in_project"] := allow("mkdir in project") if {
	input.binary_name == "mkdir"
	command_targets_in_project(input.positional.directories)
}

rules["ask_mkdir_outside_project"] := ask("mkdir outside project - confirm") if {
	input.binary_name == "mkdir"
	command_has_target_outside_project(input.positional.directories)
}

# Ask for rm -r outside project. Plenty of legitimate uses (rm -rf /tmp/foo,
# clearing ~/.cache, etc.), so a blanket deny is too aggressive — see the
# "transient trust zone" idea in IDEAS.md for a more precise classification.
rules["ask_rm_outside_project"] := ask("Recursive delete outside project - confirm target") if {
	input.binary_name == "rm"
	input.parsed_flags.recursive
	some target in input.positional.targets
	target.trust_zone != "project"
}

# Deny rm --no-preserve-root (no legitimate use case)
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
