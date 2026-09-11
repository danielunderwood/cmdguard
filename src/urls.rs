//! URL canonicalization.
//!
//! Policies match allow-list patterns against URLs, and a raw command-line
//! token is a bad thing to match: `HTTP://LOCALHOST:3000/x`,
//! `http://localhost:03000/x` and `localhost:3000/x` all reach the same
//! service while looking nothing alike, and `http://localhost:3000@evil.com/`
//! looks like localhost while reaching `evil.com`. Parse the token the way
//! curl would instead and hand policies a single canonical string.

use serde::Serialize;
use url::Url;

/// A URL reduced to the one form policies match against.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CanonicalUrl {
    /// `scheme://host[:port]/path[?query]`: scheme and host lowercased, a
    /// default port removed, dot segments resolved, fragment dropped.
    pub canonical: String,
    pub scheme: String,
    pub host: String,
    /// Absent when the port is the scheme's default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    pub path: String,
}

/// Why a raw token could not be turned into a [`CanonicalUrl`].
///
/// `reason` is short and machine-readable so policies can branch on it.
#[derive(Debug, Clone, PartialEq)]
pub struct UrlRejection {
    pub reason: String,
}

/// Characters that make the URL cmdguard checked differ from the URL curl
/// fetches, paired with the reason each one is rejected under.
///
/// `{}` and `[]` are curl's own URL globbing (unless `-g`), which also leaves
/// IPv6 literals such as `http://[::1]:3000/` unallowable; `$` and a backtick
/// are shell expansions; a backslash and whitespace make the token parse
/// differently for the shell, for cmdguard and for curl.
fn hazardous_character(raw: &str) -> Option<&'static str> {
    if raw.chars().any(|c| c.is_ascii_whitespace()) {
        return Some("whitespace");
    }
    if raw.chars().any(|c| c.is_control()) {
        return Some("control_char");
    }
    if raw.contains('\\') {
        return Some("backslash");
    }
    if raw.contains(['{', '}', '[', ']', '*']) {
        return Some("glob");
    }
    if raw.contains(['$', '`']) {
        return Some("expansion");
    }
    None
}

/// Host prefixes curl turns into a non-http scheme when the URL has none:
/// `ftp.example.com/x` is an FTP transfer, not a web request.
const GUESSED_NON_HTTP_PREFIXES: [&str; 6] = ["ftp.", "dict.", "ldap.", "imap.", "smtp.", "pop3."];

/// Does the token start with `scheme://`?
fn has_scheme(raw: &str) -> bool {
    let mut chars = raw.char_indices();
    match chars.next() {
        Some((_, c)) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for (index, c) in chars {
        if c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-' {
            continue;
        }
        return raw[index..].starts_with("://");
    }
    false
}

fn reject(reason: &str) -> UrlRejection {
    UrlRejection {
        reason: reason.to_string(),
    }
}

/// Canonicalize a raw URL token, or say why it cannot be one.
///
/// Fails closed: anything that cannot be resolved to an unambiguous http(s)
/// destination is rejected rather than approximated.
pub fn canonicalize_url(raw: &str) -> Result<CanonicalUrl, UrlRejection> {
    if raw.is_empty() {
        return Err(reject("empty"));
    }
    if let Some(reason) = hazardous_character(raw) {
        return Err(reject(reason));
    }

    // curl guesses the scheme from the host when the URL has none, so a
    // schemeless token is only http(s) when its first label is not one of the
    // hosts curl reads as another protocol.
    let with_scheme = if has_scheme(raw) {
        raw.to_string()
    } else {
        let lowered = raw.to_ascii_lowercase();
        if GUESSED_NON_HTTP_PREFIXES
            .iter()
            .any(|prefix| lowered.starts_with(prefix))
        {
            return Err(reject("scheme"));
        }
        format!("http://{raw}")
    };

    let mut url = Url::parse(&with_scheme).map_err(|_| reject("parse"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(reject("scheme"));
    }
    // `http://localhost:3000@evil.com/` reaches evil.com with `localhost` as a
    // username. Credentials in a URL have no legitimate use here.
    if !url.username().is_empty() || url.password().is_some() {
        return Err(reject("userinfo"));
    }
    // Defensive: `Url::parse` already refuses an http(s) URL without a host,
    // so this only matters if that ever stops being true.
    let host = match url.host_str() {
        Some(host) if !host.is_empty() => host.to_string(),
        _ => return Err(reject("no_host")),
    };

    // A fragment never leaves the client, so it cannot select the destination -
    // but `http://evil.com#http://localhost:3000/` would otherwise let a
    // pattern-matching policy see the allowed host.
    url.set_fragment(None);

    Ok(CanonicalUrl {
        scheme: url.scheme().to_string(),
        host,
        port: url.port(),
        path: url.path().to_string(),
        canonical: url.as_str().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_equivalent_urls_to_one_string() {
        // (raw, canonical, scheme, host, port, path)
        let cases = [
            (
                "http://localhost:3000/x",
                "http://localhost:3000/x",
                "http",
                "localhost",
                Some(3000),
                "/x",
            ),
            // Scheme and host are case-insensitive, a port is a number, dot
            // segments are path syntax, and the fragment never leaves the
            // client: all four spellings name the same request.
            (
                "HTTP://LOCALHOST:03000/a/../x?q=1#f",
                "http://localhost:3000/x?q=1",
                "http",
                "localhost",
                Some(3000),
                "/x",
            ),
            // An empty path is `/`.
            (
                "http://localhost:3000",
                "http://localhost:3000/",
                "http",
                "localhost",
                Some(3000),
                "/",
            ),
            // curl guesses `http://` for a schemeless URL, so cmdguard does too.
            (
                "localhost:3000/x",
                "http://localhost:3000/x",
                "http",
                "localhost",
                Some(3000),
                "/x",
            ),
            // A default port is not part of the destination.
            (
                "https://localhost:443/x",
                "https://localhost/x",
                "https",
                "localhost",
                None,
                "/x",
            ),
            // `127.1` is the same address as `127.0.0.1`.
            (
                "http://127.1:3000/x",
                "http://127.0.0.1:3000/x",
                "http",
                "127.0.0.1",
                Some(3000),
                "/x",
            ),
            // An internationalized host is matched in its punycode form.
            (
                "http://exämple.com/x",
                "http://xn--exmple-cua.com/x",
                "http",
                "xn--exmple-cua.com",
                None,
                "/x",
            ),
            // The fragment is dropped rather than matched: without this,
            // `^http://localhost:3000` would match a request to evil.com.
            (
                "http://evil.com#http://localhost:3000/",
                "http://evil.com/",
                "http",
                "evil.com",
                None,
                "/",
            ),
        ];

        for (raw, canonical, scheme, host, port, path) in cases {
            let url = canonicalize_url(raw).unwrap_or_else(|e| panic!("{raw} rejected: {e:?}"));
            assert_eq!(url.canonical, canonical, "canonical for {raw}");
            assert_eq!(url.scheme, scheme, "scheme for {raw}");
            assert_eq!(url.host, host, "host for {raw}");
            assert_eq!(url.port, port, "port for {raw}");
            assert_eq!(url.path, path, "path for {raw}");
        }
    }

    #[test]
    fn rejects_urls_that_cannot_be_matched_safely() {
        // (raw, reason)
        let cases = [
            ("", "empty"),
            ("http://localhost:3000/a b", "whitespace"),
            ("http://localhost:3000/a\nb", "whitespace"),
            ("http://localhost:3000/a\u{7}b", "control_char"),
            // The shell strips the backslash, so curl requests
            // `http://localhost:3000@evil.com/` - userinfo, not a host.
            ("http://localhost:3000\\@evil.com", "backslash"),
            // curl's URL globbing: one token, many requests.
            ("http://localhost:3000/{a,b}", "glob"),
            ("http://localhost:3000/[1-9].txt", "glob"),
            ("http://localhost:3000/*", "glob"),
            // Which also means an IPv6 literal can never be allowed.
            ("http://[::1]:3000/", "glob"),
            // The shell decides what these fetch.
            ("http://localhost:3000/$(id)", "expansion"),
            ("http://localhost:3000/$X", "expansion"),
            ("http://localhost:3000/`id`", "expansion"),
            // Credentials make the host something other than it looks.
            ("http://localhost:3000@evil.com/", "userinfo"),
            ("http://user:pass@localhost:3000/x", "userinfo"),
            // Not a web request.
            ("file:///etc/passwd", "scheme"),
            ("ftp://example.com/x", "scheme"),
            // curl guesses a non-http scheme from these hosts.
            ("ftp.localhost:3000/x", "scheme"),
            ("SMTP.example.com/x", "scheme"),
            // An http(s) URL has to have a host; the parser refuses these
            // before the `no_host` check ever sees them.
            ("http://", "parse"),
            ("http://:3000/x", "parse"),
            // `3000.evil.com` is not a port number, so this is not a URL at all -
            // and, whatever it is, its host is not `localhost`.
            ("http://localhost:3000.evil.com/x", "parse"),
        ];

        for (raw, reason) in cases {
            let rejection = canonicalize_url(raw)
                .err()
                .unwrap_or_else(|| panic!("{raw} was not rejected"));
            assert_eq!(rejection.reason, reason, "reason for {raw}");
        }
    }
}
