package claude.permissions

import rego.v1

rules["gh_cli"] := allow("Allowed gh command") if {
	input.command[0] == "gh"
	input.command[1] == "pr"
	input.command[2] in {"checks", "diff", "list", "view"}
}

rules["gh_cli_run"] := allow("Allowed gh command") if {
	input.command[0] == "gh"
	input.command[1] == "run"
	input.command[2] in {"list", "view"}
}

is_gh_cli if input.binary_name == "gh"

is_gh_api if {
	is_gh_cli
	input.positional_args[0].values[0].raw == "api"
}

is_gh_api_pr_files if {
	is_gh_api
	regex.match(`pulls/\d+/files$`, input.positional_args[0].values[1].raw)
}

is_gh_api_action_run if {
	is_gh_api
	regex.match(`actions/jobs/\d+/logs$`, input.positional_args[0].values[1].raw)
}

rules["gh_cli_new"] := allow("Allowed gh command") if is_gh_api_pr_files
rules["gh_cli_actions_runs"] := allow("Allowed gh command") if is_gh_api_action_run

rules["gh_issue_readonly"] := allow("Allowed gh issue command") if {
	is_gh_cli
	input.positional.args[0].raw == "issue"
	input.positional.args[1].raw == "view"
}

allowed_with_args["gh"] := {"search"}
