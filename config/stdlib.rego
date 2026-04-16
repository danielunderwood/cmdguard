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
	rule_name := sprintf("allowed_%s_%s", [binary, input.subcommand])
	reason := sprintf("Allowed %s subcommand", [binary])
}

# Declarative first-argument allowlist
# For commands without formal subcommands (e.g., npx, runners):
#   allowed_with_args["npx"] := {"tsc", "next"}
# Matches command[0] and command[1] directly, no builtins.ncl entry needed.
default allowed_with_args := {}

rules[rule_name] := allow(reason) if {
	some binary, args in allowed_with_args
	input.command[0] == binary
	input.command[1] in args
	rule_name := sprintf("allowed_%s_%s", [binary, input.command[1]])
	reason := sprintf("Allowed %s command", [binary])
}

# Priority-based rule aggregation
# Each policy file adds rules to the `rules` map
# This picks the highest priority matching rule

default result := {
	"decision": "ask",
	"reason": "No rule matched",
	"rule": "default",
	"explicit": false,
}

# Find the maximum priority among matching rules
_max_priority := max({rule.priority | some name; rule := rules[name]})

# Decision weight for tie-breaking: deny > ask > allow
_decision_weight("deny") := 3
_decision_weight("ask") := 2
_decision_weight("allow") := 1

# Among rules at max priority, find the heaviest decision weight
_max_decision_weight := max({_decision_weight(rule.decision) |
	some name; rule := rules[name]; rule.priority == _max_priority
})

# Among rules at max priority with heaviest decision, pick alphabetically first name
_winning_rule_name := name if {
	some name
	rules[name].priority == _max_priority
	_decision_weight(rules[name].decision) == _max_decision_weight
	not _earlier_name_wins(name)
}

_earlier_name_wins(name) if {
	some other
	other < name
	rules[other].priority == _max_priority
	_decision_weight(rules[other].decision) == _max_decision_weight
}

# Result from highest priority matching rule
result := {
	"decision": rules[_winning_rule_name].decision,
	"reason": rules[_winning_rule_name].reason,
	"rule": _winning_rule_name,
	"explicit": true,
} if {
	count(rules) > 0
}
