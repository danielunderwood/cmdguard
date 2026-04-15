package cmdguard

import rego.v1

# Make runs arbitrary targets from Makefile — use as a project-level policy.
# Add this to your .cmdguard/ directory to allow make in a specific project.
#
# Example: only allow specific targets
# allowed_with_args["make"] := {"build", "test", "clean", "lint"}
#
# Example: allow all make invocations
rules["allow_make"] := allow("Make allowed in this project") if {
	input.binary_name == "make"
}
