use crate::model::FlowError;
use web_sys::Url;

pub(super) fn permission(url: &str) -> Result<String, FlowError> {
    let parsed = Url::new(url).map_err(|_| invalid())?;
    if url.len() > 2_048
        || url.chars().any(char::is_control)
        || !matches!(parsed.protocol().as_str(), "http:" | "https:")
        || parsed.hostname().is_empty()
        || !parsed.username().is_empty()
        || !parsed.password().is_empty()
        || parsed.port() == "0"
        || parsed.href() != url
    {
        return Err(invalid());
    }
    // Chrome's host grant covers every port; each operation still pins the exact page URL.
    Ok(format!("{}//{}/*", parsed.protocol(), parsed.hostname()))
}

fn invalid() -> FlowError {
    FlowError::InvalidInput("Select an HTTP or HTTPS page without credentials in its URL.".into())
}

pub(super) fn removed_site(removed: &str, site: &str) -> bool {
    if removed == "<all_urls>" {
        return true;
    }
    let (Some((scheme, host)), Some((site_scheme, site_host))) = (pattern(removed), pattern(site))
    else {
        return true;
    };
    (scheme == "*" || scheme == site_scheme)
        && (host == "*"
            || host == site_host
            || host.strip_prefix("*.").is_some_and(|suffix| {
                site_host == suffix
                    || site_host
                        .strip_suffix(suffix)
                        .is_some_and(|prefix| prefix.ends_with('.'))
            }))
}

fn pattern(value: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = value.split_once("://")?;
    let authority = rest.split_once('/')?.0;
    let host = if authority.starts_with('[') {
        &authority[..=authority.find(']')?]
    } else {
        authority.split(':').next()?
    };
    (!host.is_empty()).then_some((scheme, host))
}

#[cfg(test)]
mod checks {
    use super::*;

    #[test]
    fn revocation_invalidates_overlapping_sites_without_matching_host_suffixes() {
        for removed in [
            "<all_urls>",
            "*://*/*",
            "https://*.example.com/*",
            "https://app.example.com:8443/*",
        ] {
            assert!(removed_site(removed, "https://app.example.com/*"));
        }
        for removed in [
            "http://app.example.com/*",
            "https://example.org/*",
            "https://*.ample.com/*",
        ] {
            assert!(!removed_site(removed, "https://app.example.com/*"));
        }
        assert!(removed_site(
            "https://*.example.com/*",
            "https://example.com/*"
        ));
        assert!(!removed_site(
            "https://*.example.com/*",
            "https://badexample.com/*"
        ));
        assert!(removed_site("http://[::1]:8443/*", "http://[::1]/*"));
        assert!(!removed_site("http://[::2]/*", "http://[::1]/*"));
    }
}
