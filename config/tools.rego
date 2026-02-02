package claude.permissions

import rego.v1

in_bin_path(name) if {
    some path in ["", "./target/release/", "./target/debug/"]
    input.command[0] == sprintf("%s%s", [path, name])
}

rules["local_claude_permissions"] := {
    "decision": "allow",
    "reason": "Allowed local tool",
    "priority": 25,
} if in_bin_path("claude-permissions")
