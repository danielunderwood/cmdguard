package claude.permissions

import rego.v1

rules["safe_command"] := {
    "decision": "allow",
    "reason": "Safe command",
    "priority": 25,
} if {
    input.command[0] in {"ls", "echo", "cat", "grep", "head", "tail", "rg", "mkdir", "touch", "file"}
}
