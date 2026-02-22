package claude.permissions

import rego.v1

allowed_subcommands["git"] := {
	"check-ignore",
	"diff",
	"fetch",
	"log",
	"ls-tree",
	"rev-parse",
	"show",
	"status",
	"version",
}

rules["allow_git_help"] := allow("Allowed git town") if {
	input.binary_name == "git"
	input.positional.args[0].raw == "town"
	input.positional.args[1].raw == "help"
}

# Deny git push --force using parsed_flags
rules["force_push_structured"] := deny("Force push blocked (detected via parsed_flags)") if {
	input.subcommand == "push"
	input.parsed_flags.force == true
}

# Deny git reset --hard using parsed_flags
rules["hard_reset"] := deny("Hard reset blocked - use --soft or --mixed instead") if {
	input.subcommand == "reset"
	input.parsed_flags.hard == true
}

# Deny git clean -x (removes ignored files)
rules["clean_ignored"] := ask("git clean -x removes ignored files - please confirm") if {
	input.subcommand == "clean"
	input.parsed_flags.ignored == true
}

rules["git_worktree"] := allow("git worktree") if {
	input.binary_name == "git"
	input.subcommand == "worktree"
	input.positional.subcommand[0].raw == "list"
}

is_git_branch if {
	input.binary_name == "git"
	input.subcommand == "branch"
}

git_branch_allowed_flag if "--show-current" in input.flags_expanded
git_branch_allowed_flag if "-a" in input.flags_expanded

# Just "git branch" or "git branch --list"
git_branch_allowed_flag if {
	is_git_branch
	input.flags_expanded in {[], ["--list"]}
	input.positional.args == []
}

rules["git_branch_readonly"] := allow("git branch readonly") if {
	is_git_branch
	git_branch_allowed_flag
}
