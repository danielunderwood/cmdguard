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
# Allow shell output redirects to specific targets without suppressing other
# matching ask rules:
#   allowed_redirect_targets["/dev/null"] := true
#
# Allow curl when every declared URL matches a regular expression (matched from
# the start of the URL; end it with ($|/) so the host cannot be extended):
#   allowed_curl_patterns contains `^http://localhost:3000($|/)`
#
# Add a conditional rule:
#   rules["my_rule"] := ask("Please confirm") if {
#       input.binary_name == "dangerous-tool"
#   }
