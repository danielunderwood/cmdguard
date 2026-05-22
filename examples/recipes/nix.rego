package cmdguard

import rego.v1

is_nix if input.command[0] == "nix"

is_nix_flake if {
	is_nix
	input.command[1] == "flake"
}

rules["allowed_nix"] := allow("Allowed nix command") if {
	is_nix
	input.command[1] in {"build", "version"}
}

rules["allowed_flake"] := allow("Allowed flake command") if {
	is_nix_flake
	input.command[2] in {"check", "info", "show", "update"}
}

rules["allowed_nh"] := allow("Allowed nh command") if {
	input.command[0] == "nh"
	input.command[1] == "search"
}
