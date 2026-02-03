package claude.permissions

import rego.v1

# Parsed command fields available in input:
#   input.parsed_flags - Object with flag names and values (e.g., {force: true, user: "root"})
#   input.positional_args - Array of {name, values: [{raw, resolved, trust_zone, type}]}
#   input.subcommand - Detected subcommand (e.g., "push" for "git push")

rules["safe_git"] := {
    "decision": "allow",
    "reason": "Safe git command",
    "priority": 25,
} if {
    input.command[0] == "git"
    input.command[1] in {"status", "log", "ls-tree", "show", "version", "diff"}
}

# Deny git push --force using parsed_flags
rules["force_push_structured"] := {
    "decision": "deny",
    "reason": "Force push blocked (detected via parsed_flags)",
    "priority": 100,
} if {
    input.subcommand == "push"
    input.parsed_flags.force == true
}

# Deny git reset --hard using parsed_flags
rules["hard_reset"] := {
    "decision": "deny",
    "reason": "Hard reset blocked - use --soft or --mixed instead",
    "priority": 100,
} if {
    input.subcommand == "reset"
    input.parsed_flags.hard == true
}

# Deny git clean -x (removes ignored files)
rules["clean_ignored"] := {
    "decision": "ask",
    "reason": "git clean -x removes ignored files - please confirm",
    "priority": 75,
} if {
    input.subcommand == "clean"
    input.parsed_flags.ignored == true
}
