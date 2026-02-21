package claude.permissions

import rego.v1

rules["kubectl_readonly"] := allow("kubectl readonly") if {
	input.binary_name == "kubectl"
	input.positional.args[0].raw in {"get", "describe", "logs", "rollout"}
	not startswith(input.positional.args[1].raw, "secret")
}

rules["helm_readonly"] := allow("helm readonly") if {
	input.binary_name == "helm"
	input.positional.args[0].raw in {"get", "history", "list", "show", "view"}
}

rules["flux"] := allow("flux") if {
	input.binary_name == "flux"
	input.positional.args[0].raw in {"get", "reconcile"}
}
