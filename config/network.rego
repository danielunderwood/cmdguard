package cmdguard

import rego.v1

# Users can allow declared curl URLs by contributing anchored regular
# expressions from any custom policy file:
#
#   allowed_curl_patterns contains `^http://localhost:3000($|/)`
#
# Keep this empty by default so curl continues to ask until configured.
default allowed_curl_patterns := set()

_curl_has_url if {
	some url in object.get(input.positional, "url", [])
}

_curl_has_url if {
	some url in object.get(input.parsed_flags, "url", [])
}

_curl_url_matches_allowed_pattern(url) if {
	some pattern in allowed_curl_patterns
	regex.match(pattern, url)
}

_curl_urls_allowed if {
	every url in object.get(input.positional, "url", []) {
		_curl_url_matches_allowed_pattern(url.raw)
	}
	every url in object.get(input.parsed_flags, "url", []) {
		_curl_url_matches_allowed_pattern(url)
	}
}

_curl_uses_destination_indirection if {
	some flag in {
		"location",
		"location_trusted",
		"config",
		"connect_to",
		"resolve",
		"proxy",
		"preproxy",
		"unix_socket",
		"abstract_unix_socket",
	}
	object.get(input.parsed_flags, flag, false) != false
}

# The normal allow priority (25) beats curl's informational ask (20), while
# unrelated safety asks such as shell output redirection (50) still win.
rules["allow_curl_patterns"] := allow("curl URLs match allowed patterns") if {
	input.binary_name == "curl"
	_curl_has_url
	_curl_urls_allowed
	not _curl_uses_destination_indirection
}

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
