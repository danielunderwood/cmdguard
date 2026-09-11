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
# Allow shell output redirects to specific absolute nominal targets without
# suppressing other ask rules. Every candidate must be listed when cwd is
# ambiguous; unknown targets still ask:
#   allowed_redirect_targets["/dev/null"] := true
#
# Allow curl when every declared URL matches a regular expression. Patterns
# are matched from the start of the URL's canonical form --
# `scheme://host[:port]/path`, scheme and host lowercased, a default port and
# the fragment removed -- so end a host pattern with ($|/), or
# `http://localhost:30000/` matches it too:
#   allowed_curl_patterns contains `^http://localhost:3000($|/)`
#
# Add a conditional rule:
#   rules["my_rule"] := ask("Please confirm") if {
#       input.binary_name == "dangerous-tool"
#   }
