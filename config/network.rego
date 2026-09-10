package cmdguard

import rego.v1

# Users can allow the URLs a curl command declares by contributing regular
# expressions from any custom policy file:
#
#   allowed_curl_patterns contains `^http://localhost:3000($|/)`
#
# Patterns are anchored at the start before matching (see
# `_curl_url_matches_allowed_pattern`), so end a host pattern with `($|/)` to
# stop `http://localhost:3000.evil.example/` from matching it.
#
# Keep this empty by default so curl continues to ask until configured.
default allowed_curl_patterns := set()

_curl_has_url if {
	some url in object.get(input.positional, "url", [])
}

_curl_has_url if {
	some url in object.get(input.parsed_flags, "url", [])
}

# `regex.match` searches the whole string, so anchor the author's pattern at the
# start before using it. Without this, `localhost:3000` would also match
# `http://evil.example/?q=localhost:3000`. The pattern goes in a non-capturing
# group so that a top-level alternation stays anchored too.
_curl_url_matches_allowed_pattern(url) if {
	some pattern in allowed_curl_patterns
	regex.match(concat("", ["^(?:", pattern, ")"]), url)
}

# Characters that make the URL cmdguard checked differ from the URL curl
# fetches: `$` and backticks are shell expansions, a leading `~` is tilde
# expansion, and curl's own URL globbing expands `{a,b}` and `[1-9]`. `?` is
# left out on purpose - it is how every query string starts.
_curl_url_is_dynamic(url) if {
	some character in ["$", "`", "{", "}", "[", "]", "*"]
	contains(url, character)
}

_curl_url_is_dynamic(url) if {
	startswith(url, "~")
}

_curl_url_allowed(url) if {
	not _curl_url_is_dynamic(url)
	_curl_url_matches_allowed_pattern(url)
}

_curl_urls_allowed if {
	every url in object.get(input.positional, "url", []) {
		_curl_url_allowed(url.raw)
	}
	every url in object.get(input.parsed_flags, "url", []) {
		_curl_url_allowed(url)
	}
}

# `--url` takes the following token as its value even when that token starts
# with a dash, while cmdguard's parser refuses to consume such a token. The URL
# curl would request is then invisible here, so treat it as unchecked.
_curl_url_flag_value_hidden if {
	some i
	input.command[i] == "--url"
	startswith(input.command[i + 1], "-")
}

# Options that change where the request goes or where its settings come from.
_curl_indirection_flags := {
	"location",
	"location_trusted",
	"proto",
	"proto_redir",
	"proto_default",
	"config",
	"connect_to",
	"resolve",
	"proxy",
	"preproxy",
	"socks4",
	"socks4a",
	"socks5",
	"socks5_hostname",
	"doh_url",
	"unix_socket",
	"abstract_unix_socket",
	"interface",
}

# Options that write or read a local file. Reading one matters as much as
# writing one: the file's contents leave in the request.
_curl_local_file_flags := {
	"output",
	"remote_name",
	"remote_header_name",
	"remote_name_all",
	"output_dir",
	"create_dirs",
	"dump_header",
	"cookie_jar",
	"trace",
	"trace_ascii",
	"stderr",
	"etag_save",
	"upload_file",
	"cookie",
	"etag_compare",
	"netrc",
	"netrc_file",
	"cacert",
	"capath",
	"cert",
	"key",
}

# Options whose value curl reads from a local file when it starts with `@`.
# `--data-raw` never does, but asking is cheaper than carrying the exception.
_curl_at_file_flags := {
	"header",
	"data",
	"data_ascii",
	"data_binary",
	"data_raw",
	"form",
	"form_string",
	"write_out",
}

_curl_flag_present(flag) if {
	object.get(input.parsed_flags, flag, false) != false
}

# A flag's values, whatever shape the parser recorded: an array for a
# repeatable flag, a string for a single-valued one, and nothing for a flag
# that was present without a usable value.
_curl_flag_values(flag) := values if {
	values := object.get(input.parsed_flags, flag, [])
	is_array(values)
}

_curl_flag_values(flag) := [value] if {
	value := object.get(input.parsed_flags, flag, [])
	is_string(value)
}

_curl_flag_values(flag) := [] if {
	value := object.get(input.parsed_flags, flag, [])
	not is_array(value)
	not is_string(value)
}

_curl_uses_destination_indirection if {
	some flag in _curl_indirection_flags
	_curl_flag_present(flag)
}

_curl_touches_local_files if {
	some flag in _curl_local_file_flags
	_curl_flag_present(flag)
}

_curl_touches_local_files if {
	some flag in _curl_at_file_flags
	some value in _curl_flag_values(flag)
	startswith(value, "@")
}

# `--data-urlencode` also reads a file from the `name@file` form, not just `@file`.
_curl_touches_local_files if {
	some value in _curl_flag_values("data_urlencode")
	contains(value, "@")
}

# The normal allow priority (25) beats curl's informational ask (20), while
# unrelated safety asks such as shell output redirection (50) still win.
#
# The allow covers the URLs written on the command line only, so every other way
# of reaching a destination, of reading or writing a local file, and of
# smuggling an option past the parser has to be excluded first.
rules["allow_curl_patterns"] := allow("curl URLs match allowed patterns") if {
	input.binary_name == "curl"
	no_unknown_flags

	# An env prefix (`http_proxy=...`), `env`, `sudo` or any other wrapper can
	# change where the request goes without touching curl's own options.
	count(input.wrapper_chain) == 0
	_curl_has_url
	_curl_urls_allowed
	not _curl_url_flag_value_hidden
	not _curl_uses_destination_indirection
	not _curl_touches_local_files
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
