package claude.permissions

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

# Priority-based rule aggregation
# Each policy file adds rules to the `rules` map
# This picks the highest priority matching rule

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
