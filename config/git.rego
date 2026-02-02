package claude.permissions

import rego.v1

rules["safe_git"] := {
    "decision": "allow",
    "reason": "Safe git command",
    "priority": 25,
} if {
    input.command[0] == "git"
    input.command[1] in {"status", "log", "ls-tree", "show", "version", "diff"}
}
