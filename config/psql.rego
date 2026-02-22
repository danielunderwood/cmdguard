package claude.permissions

import rego.v1

rules["allowed_psql"] := allow("Allowed psql command") if {
	input.command[0] == "psql"
	input.command[1] == "-h"
	input.command[2] == "localhost"
}
