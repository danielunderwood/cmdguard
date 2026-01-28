package claude.permissions

import data.claude.permissions.stdlib
import future.keywords.in

default decision = "ask"

decision = "allow" {
    input.command[0] == "git"
    data.claude.permissions.stdlib.git_subcommand in {"status", "diff", "log", "branch", "show", "fetch", "stash"}
}

decision = "deny" {
    input.command[0] == "git"
    data.claude.permissions.stdlib.git_subcommand == "push"
    "--force" in input.command
}

reason = "Safe git read operation" {
    decision == "allow"
    input.command[0] == "git"
}

reason = "Force push blocked by policy" {
    input.command[0] == "git"
    data.claude.permissions.stdlib.git_subcommand == "push"
    "--force" in input.command
}
