package cmdguard

import future.keywords.in
import future.keywords.if

# Rules collection - each rule adds to this map
rules["safe_git_read"] := {
    "decision": "allow",
    "reason": "Safe git read operation",
    "priority": 25,
} if {
    input.command[0] == "git"
    input.command[1] in {"status", "diff", "log", "branch", "show", "fetch", "stash"}
}

rules["force_push_blocked"] := {
    "decision": "deny",
    "reason": "Force push blocked by policy",
    "priority": 100,
} if {
    input.command[0] == "git"
    input.command[1] == "push"
    "--force" in input.command
}

# Aggregation: collect all matching rules, pick highest priority
default result := {
    "decision": "defer",
    "reason": "No rule matched",
    "rule": "default",
    "explicit": false,
}

# Helper to find the highest priority rule
highest_priority_rule := rule if {
    some name
    rule := rules[name]
    not _higher_priority_exists(rule.priority)
}

_higher_priority_exists(p) if {
    some name
    rules[name].priority > p
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

# Get the name of the winning rule
_winning_rule_name := name if {
    some name
    rules[name] == highest_priority_rule
}
