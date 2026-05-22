package cmdguard

import rego.v1

allowed_subcommands["docker"] := {"build", "images", "logs", "ps", "pull"}

# docker run and docker exec are treated as wrappers by the parser,
# so we detect them via wrapper_chain instead of binary_name/subcommand.
rules["docker_run"] := ask("Docker run - confirm container execution") if {
	"docker run" in input.wrapper_chain
}

rules["docker_exec"] := ask("Docker exec - confirm container access") if {
	"docker exec" in input.wrapper_chain
}

rules["ask_docker_push"] := ask("Docker push - confirm registry target") if {
	input.binary_name == "docker"
	input.subcommand == "push"
}
