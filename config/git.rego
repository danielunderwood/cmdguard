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

# Allow push without force
rules["allow_git_push"] := allow("git push") if {
	input.binary_name == "git"
	input.subcommand == "push"
	not input.parsed_flags.force
}

# Deny force push
rules["deny_force_push"] := deny("Force push blocked") if {
	input.subcommand == "push"
	input.parsed_flags.force == true
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
