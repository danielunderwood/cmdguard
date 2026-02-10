package claude.permissions

import rego.v1

# Parsed command fields available in input:
#   input.parsed_flags - Object with flag names and values (e.g., {force: true, user: "root"})
#   input.positional_args - Array of {name, values: [{raw, resolved, trust_zone, type}]}
#   input.subcommand - Detected subcommand (e.g., "push" for "git push")
safe_git_commands := {
	"check-ignore",
	"diff",
	"fetch",
	"log",
	"ls-tree",
	"rev-parse",
	"show",
	"status",
	"version",
}

# This should be superceded by the new format, but keeping
# just in case
# rules["safe_git"] := {
# 	"decision": "allow",
# 	"reason": "Safe git command",
# 	"priority": 25,
# } if {
# 	input.command[0] == "git"
# 	input.command[1] in safe_git_commands
# }

rules["safe_git_parsed_args"] := {
	"decision": "allow",
	"reason": "Safe parsed git command",
	"priority": 25,
} if {
	input.binary_name == "git"
	input.subcommand in safe_git_commands
}

rules["allow_git_help"] := {
	"decision": "allow",
	"reason": "Allowed git town",
	"priority": 25,
} if {
	input.binary_name == "git"
	input.positional.args[0].raw == "town"
	input.positional.args[1].raw == "help"
}

# Deny git push --force using parsed_flags
rules["force_push_structured"] := {
	"decision": "deny",
	"reason": "Force push blocked (detected via parsed_flags)",
	"priority": 100,
} if {
	input.subcommand == "push"
	input.parsed_flags.force == true
}

# Deny git reset --hard using parsed_flags
rules["hard_reset"] := {
	"decision": "deny",
	"reason": "Hard reset blocked - use --soft or --mixed instead",
	"priority": 100,
} if {
	input.subcommand == "reset"
	input.parsed_flags.hard == true
}

# Deny git clean -x (removes ignored files)
rules["clean_ignored"] := {
	"decision": "ask",
	"reason": "git clean -x removes ignored files - please confirm",
	"priority": 75,
} if {
	input.subcommand == "clean"
	input.parsed_flags.ignored == true
}
