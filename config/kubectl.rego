package claude.permissions

import rego.v1

allowed_with_args["kubectl"] := {"kustomize"}

rules["kubectl_readonly"] := allow("kubectl readonly") if {
	input.binary_name == "kubectl"
	input.positional.args[0].raw in {"get", "describe", "logs", "rollout"}
	not startswith(input.positional.args[1].raw, "secret")
}

allowed_with_args["helm"] := {"get", "history", "list", "show", "view"}
allowed_with_args["flux"] := {"get", "reconcile"}
