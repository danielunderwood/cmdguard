package claude.permissions

import rego.v1

rules["safe_opa"] := {
    "decision": "allow",
    "reason": "Allowed opa command",
    "priority": 25,
} if {
    input.command[0] == "opa"
    input.command[1] in {
        "eval",
        "exec",
        "fmt",
        "help",
        "parse",
        "test",
        "version",
    }
}
