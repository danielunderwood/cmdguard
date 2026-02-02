package claude.permissions

import rego.v1

rules["allowed_cargo"] := {
    "decision": "allow",
    "reason": "Allowed cargo command",
    "priority": 25,
} if {
    input.command[0] == "cargo"
    input.command[1] in {"run", "test", "build", "doc", "version", "fmt", "search"}
}
