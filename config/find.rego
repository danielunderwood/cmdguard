package claude.permissions

import rego.v1

is_find if input.command[0] == "find"

find_with_exec if {
    is_find
    "-exec" in input.command
}

rules["safe_find"] := {
    "decision": "allow",
    "reason": "Allowed find command",
    "priority": 25,
} if {
    is_find
    not find_with_exec
}

rules["find_with_exec"] := {
    "decision": "ask",
    "reason": "Find command with -exec requires approval",
    "priority": 50,
} if find_with_exec
