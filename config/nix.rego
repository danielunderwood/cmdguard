package claude.permissions

import rego.v1

is_nix if input.command[0] == "nix"

is_nix_flake if {
    is_nix
    input.command[1] == "flake"
}

rules["allowed_nix"] := {
    "decision": "allow",
    "reason": "Allowed nix command",
    "priority": 25,
} if {
    is_nix
    input.command[1] in {"build", "version"}
}

rules["allowed_flake"] := {
    "decision": "allow",
    "reason": "Allowed flake command",
    "priority": 25,
} if {
    is_nix_flake
    input.command[2] in {"check", "info", "show", "update"}
}
