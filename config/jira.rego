package claude.permissions

import rego.v1

allowed_with_args["jira"] := {"help", "me", "move"}

rules["jira_command_help"] := allow("jira command help") if {
	input.binary_name == "jira"
	input.parsed_flags.help
}

rules["jira_epic"] := allow("Jira epic read") if {
	input.binary_name == "jira"
	input.positional.args[0].raw == "epic"
	input.positional.args[1].raw in {"list", "view"}
}
