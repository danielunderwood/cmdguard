package cmdguard

import rego.v1

allowed_subcommands["git"] := {
	"add",
	"branch",
	"check-ignore",
	"checkout",
	"commit",
	"diff",
	"fetch",
	"grep",
	"log",
	"ls-files",
	"ls-tree",
	"pull",
	"restore",
	"rev-parse",
	"show",
	"stash",
	"status",
	"switch",
	"version",
}

is_force_push if input.parsed_flags.force == true

is_force_push if input.parsed_flags.force_with_lease == true

# Pushing changes remote state and Claude Code auto mode treats direct pushes
# to default branches as risky. cmdguard does not know the current/default
# branch from the shell command alone, so prompt unless a user policy narrows
# this to a known-safe branch workflow.
rules["ask_git_push"] := ask("git push changes remote state - confirm branch") if {
	input.binary_name == "git"
	input.subcommand == "push"
	not is_force_push
}

# Force pushes are destructive but sometimes intended; ask the user.
# --force-with-lease is preferred over --force when one is needed.
rules["ask_force_push"] := ask("Force push - prefer --force-with-lease over --force") if {
	input.binary_name == "git"
	input.subcommand == "push"
	input.parsed_flags.force == true
	not input.parsed_flags.force_with_lease == true
}

rules["ask_force_with_lease_push"] := ask("Force-with-lease push - confirm overwrite") if {
	input.binary_name == "git"
	input.subcommand == "push"
	input.parsed_flags.force_with_lease == true
}

# Ask for reset --hard
rules["ask_git_reset_hard"] := ask("git reset --hard discards changes") if {
	input.binary_name == "git"
	input.subcommand == "reset"
	input.parsed_flags.hard == true
}

# Ask for clean
rules["ask_git_clean"] := ask("git clean removes untracked files") if {
	input.binary_name == "git"
	input.subcommand == "clean"
}

is_git_branch if {
	input.binary_name == "git"
	input.subcommand == "branch"
}

git_branch_allowed_flag if "--show-current" in input.flags_expanded
git_branch_allowed_flag if "-a" in input.flags_expanded

git_branch_allowed_flag if {
	is_git_branch
	input.flags_expanded in {[], ["--list"]}
	input.positional.args == []
}

rules["git_branch_readonly"] := allow("git branch readonly") if {
	is_git_branch
	git_branch_allowed_flag
}

rules["git_worktree_list"] := allow("git worktree list") if {
	input.binary_name == "git"
	input.subcommand == "worktree"
	input.positional.subcommand[0].raw == "list"
}
