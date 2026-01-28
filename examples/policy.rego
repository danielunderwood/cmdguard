package claude.permissions

import data.claude.permissions.stdlib
import future.keywords.in

default decision = "ask"

# =============================================================================
# ALLOW RULES
# =============================================================================

# Allow safe git read commands
decision = "allow" {
    input.command[0] == "git"
    data.claude.permissions.stdlib.git_subcommand in {
        "status", "diff", "log", "branch", "show",
        "fetch", "stash", "remote", "tag", "describe"
    }
}

# Allow git add/commit (common safe operations)
decision = "allow" {
    input.command[0] == "git"
    data.claude.permissions.stdlib.git_subcommand in {"add", "commit", "restore", "switch", "checkout"}
}

# Allow cargo commands
decision = "allow" {
    input.command[0] == "cargo"
    input.command[1] in {"build", "test", "check", "fmt", "clippy", "run", "doc"}
}

# Allow npm/yarn/pnpm safe commands
decision = "allow" {
    input.command[0] in {"npm", "yarn", "pnpm"}
    input.command[1] in {"install", "run", "test", "build", "start", "dev"}
}

# Allow common read-only commands
decision = "allow" {
    input.command[0] in {"ls", "cat", "head", "tail", "grep", "find", "wc", "file", "which"}
}

# Allow echo and printf
decision = "allow" {
    input.command[0] in {"echo", "printf"}
}

# =============================================================================
# DENY RULES
# =============================================================================

# Deny git push --force
decision = "deny" {
    input.command[0] == "git"
    data.claude.permissions.stdlib.git_subcommand == "push"
    has_force_flag
}

has_force_flag {
    some flag in input.command
    flag in {"--force", "-f", "--force-with-lease"}
}

# Deny rm -rf outside project root
decision = "deny" {
    input.command[0] == "rm"
    "-r" in input.flags_expanded
    data.claude.permissions.stdlib.path_outside_project
}

# Deny dangerous commands entirely
decision = "deny" {
    input.command[0] in {"shutdown", "reboot", "mkfs", "dd"}
}

# =============================================================================
# REASONS
# =============================================================================

reason = "Safe git read operation" {
    decision == "allow"
    input.command[0] == "git"
}

reason = "Safe cargo operation" {
    decision == "allow"
    input.command[0] == "cargo"
}

reason = "Safe package manager operation" {
    decision == "allow"
    input.command[0] in {"npm", "yarn", "pnpm"}
}

reason = "Read-only command" {
    decision == "allow"
    input.command[0] in {"ls", "cat", "head", "tail", "grep", "find", "wc", "file", "which", "echo", "printf"}
}

reason = "Force push is blocked - use regular push instead" {
    decision == "deny"
    input.command[0] == "git"
    data.claude.permissions.stdlib.git_subcommand == "push"
}

reason = "Recursive delete outside project root is blocked" {
    decision == "deny"
    input.command[0] == "rm"
    "-r" in input.flags_expanded
}

reason = "This command is blocked for safety" {
    decision == "deny"
    input.command[0] in {"shutdown", "reboot", "mkfs", "dd"}
}
