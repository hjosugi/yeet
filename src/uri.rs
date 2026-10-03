//! The URI shapes the drop and clipboard paths care about.
//!
//! Both the model and the GTK layer have to recognise a web URL and a local
//! `file://` URL, and both have to accept them case-insensitively because the
//! scheme is case-insensitive and drag sources are inconsistent about it.

/// Whether `uri` names an `http` or `https` resource.
pub fn is_web_uri(uri: &str) -> bool {
    scheme_matches(uri, "http://") || scheme_matches(uri, "https://")
}

/// Whether `uri` names a local file through the `file` scheme.
pub fn is_file_uri(uri: &str) -> bool {
    scheme_matches(uri, "file://")
}

/// Compare the leading scheme without allocating or lower-casing the whole URI.
fn scheme_matches(uri: &str, scheme: &str) -> bool {
    uri.get(..scheme.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_schemes_are_recognised_case_insensitively() {
        assert!(is_web_uri("http://example.com"));
        assert!(is_web_uri("HTTPS://example.com/file"));
        assert!(!is_web_uri("ftp://example.com/file"));
        assert!(!is_web_uri("file:///tmp/file"));
        assert!(!is_web_uri("https:/short"));
    }

    #[test]
    fn file_scheme_is_recognised_case_insensitively() {
        assert!(is_file_uri("file:///tmp/file"));
        assert!(is_file_uri("FILE://server/share"));
        assert!(!is_file_uri("https://example.com/file"));
    }
}
