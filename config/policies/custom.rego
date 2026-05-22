package cmdguard

import rego.v1

# Add your custom rules here. These override base rules via priority.
#
# Examples:
#
# Deny a subcommand that base allows:
#   denied_subcommands["git"] := {"push"}
#
# Add an allow rule for a tool not in base:
#   allowed_with_args["make"] := {"build", "test", "clean"}
#
# Add a conditional rule:
#   rules["my_rule"] := ask("Please confirm") if {
#       input.binary_name == "dangerous-tool"
#   }
