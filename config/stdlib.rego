package cmdguard

import rego.v1

flag_value(flag) := value if {
	some i
	input.command[i] == flag
	value := input.command[i + 1]
}

git_subcommand := input.command[1] if {
	input.command[0] == "git"
	count(input.command) > 1
	not startswith(input.command[1], "-")
}

path_outside_project if {
	some path in input.paths
	not startswith(path.resolved, input.project_root)
}

all_paths_in_project if {
	count(input.paths) > 0
	every path in input.paths {
		startswith(path.resolved, input.project_root)
	}
}

no_paths if {
	count(input.paths) == 0
}

# Decision helpers - reduces per-rule boilerplate
allow(reason) := {"decision": "allow", "reason": reason, "priority": 25}
deny(reason) := {"decision": "deny", "reason": reason, "priority": 100}
ask(reason) := {"decision": "ask", "reason": reason, "priority": 50}

# Custom-priority variants
allow_at(reason, priority) := {"decision": "allow", "reason": reason, "priority": priority}
deny_at(reason, priority) := {"decision": "deny", "reason": reason, "priority": priority}
ask_at(reason, priority) := {"decision": "ask", "reason": reason, "priority": priority}

# Declarative subcommand allowlist
# Policy files contribute entries via partial object rules:
#   allowed_subcommands["git"] := {"status", "log", "diff"}
#   allowed_subcommands["cargo"] := {"build", "test", "check"}
default allowed_subcommands := {}

rules[rule_name] := allow(reason) if {
	some binary, subcmds in allowed_subcommands
	input.binary_name == binary
	input.subcommand in subcmds
	not _subcommand_denied(binary, input.subcommand)
	rule_name := sprintf("allowed_%s_%s", [binary, input.subcommand])
	reason := sprintf("Allowed %s subcommand", [binary])
}

# Declarative first-argument allowlist
# For commands without formal subcommands (e.g., npx, runners):
#   allowed_with_args["npx"] := {"tsc", "next"}
# Matches command[0] and command[1] directly, no builtins.ncl entry needed.
default allowed_with_args := {}

# Exclusion tables — users add entries to narrow base allow-lists
default denied_subcommands := {}
default denied_with_args := {}

_subcommand_denied(binary, subcmd) if {
	some denied_binary, denied_set in denied_subcommands
	binary == denied_binary
	subcmd in denied_set
}

_with_args_denied(binary, arg) if {
	some denied_binary, denied_set in denied_with_args
	binary == denied_binary
	arg in denied_set
}

rules[rule_name] := allow(reason) if {
	some binary, args in allowed_with_args
	input.command[0] == binary
	input.command[1] in args
	not _with_args_denied(binary, input.command[1])
	rule_name := sprintf("allowed_%s_%s", [binary, input.command[1]])
	reason := sprintf("Allowed %s command", [binary])
}

# Priority-based rule aggregation
# Each policy file adds rules to the `rules` map
# This picks the highest priority matching rule

# Helper to expose all matching rules for debugging (used by eval command)
all_rules := rules

default result := {
	"decision": "ask",
	"reason": "No rule matched",
	"rule": "default",
	"explicit": false,
}

# Find the rule with highest priority
highest_priority_rule := rule if {
	some name
	rule := rules[name]
	not _higher_priority_exists(rule.priority)
}

_higher_priority_exists(p) if {
	some name
	rules[name].priority > p
}

# Get the name of the winning rule
_winning_rule_name := name if {
	some name
	rules[name] == highest_priority_rule
}

# Result from highest priority matching rule
result := {
	"decision": highest_priority_rule.decision,
	"reason": highest_priority_rule.reason,
	"rule": _winning_rule_name,
	"explicit": true,
} if {
	count(rules) > 0
}
