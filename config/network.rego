package cmdguard

import rego.v1

# curl/wget ask by default with informative message
rules["curl_ask"] := ask_at("curl - confirm URL", 20) if {
	input.binary_name == "curl"
}

rules["wget_ask"] := ask_at("wget - confirm URL", 20) if {
	input.binary_name == "wget"
}

# Deny wget --recursive (can download entire sites)
rules["deny_wget_recursive"] := deny("Recursive wget blocked") if {
	input.binary_name == "wget"
	input.parsed_flags.recursive
}

# Deny rsync --delete
rules["deny_rsync_delete"] := deny("rsync --delete blocked by default") if {
	input.binary_name == "rsync"
	input.parsed_flags.delete
}
