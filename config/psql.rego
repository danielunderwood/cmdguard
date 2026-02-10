package claude.permissions

import rego.v1

rules["allowed_psql"] := {
	"decision": "allow",
	"reason": "Allowed psql command",
	"priority": 25,
} if {
	input.command[0] == "psql"
	input.command[1] == "-h"
	input.command[2] == "localhost"
}
