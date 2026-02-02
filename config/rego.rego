package claude.permissions

import rego.v1

rules["regal"] := {
    "decision": "allow",
    "reason": "Regal commands allowed",
    "priority": 25,
} if {
    input.command[0] == "regal"
}
