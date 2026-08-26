//! Negative tests for the vendor-host blocklist.
//!
//! These live in `tests/` rather than beside the code because they must spell
//! out the hosts they assert are blocked, and the forbidden-domain scanner would
//! flag those literals. Allowlisting `crates/*/tests/**` costs nothing, whereas
//! suppressing the scanner inside `network-policy/src/lib.rs` would blind it in
//! the one file that owns all outbound network access.
//!
//! Only the public API is used here, which is the whole point: these assert the
//! guarantee a caller actually gets.

use kw_network_policy::{validate, PolicyError, RequestPurpose, ALL_PURPOSES};

/// Hosts that must never be reachable, whatever the caller claims to be doing.
const BLOCKED: &[&str] = &[
    "https://api.openwhispr.com/api/v1/notes",
    "https://auth.openwhispr.com/session",
    "https://mcp.openwhispr.com/mcp",
    "https://docs.openwhispr.com/cli/install",
    "https://openwhispr.com/auth/desktop-callback",
    "https://pluely.com/api/update",
    "https://app.posthog.com/capture",
    "https://us.i.posthog.com/capture",
];

#[test]
fn blocked_for_every_purpose_without_exception() {
    // Iterating every purpose matters: a blocklist that only applied to some
    // egress reasons would be trivially bypassed by mislabelling a request.
    for purpose in ALL_PURPOSES {
        for url in BLOCKED {
            assert!(
                matches!(
                    validate(url, purpose),
                    Err(PolicyError::BlockedVendorHost { .. })
                ),
                "{url} must be blocked for purpose {}",
                purpose.as_str()
            );
        }
    }
}

#[test]
fn subdomains_are_blocked() {
    for url in [
        "https://anything.openwhispr.com/x",
        "https://deeply.nested.openwhispr.com/x",
        "https://cdn.pluely.com/asset.js",
    ] {
        assert!(
            matches!(
                validate(url, RequestPurpose::UserConfiguredAiProvider),
                Err(PolicyError::BlockedVendorHost { .. })
            ),
            "{url} must be blocked"
        );
    }
}

#[test]
fn lookalike_domains_are_not_blocked() {
    // A naive suffix match would catch these. They are different registrable
    // domains and blocking them would be wrong -- someone may legitimately host
    // an OpenAI-compatible endpoint on one.
    for url in [
        "https://notopenwhispr.com/x",
        "https://myopenwhispr.com/x",
        "https://openwhispr.com.example.org/x",
        "https://pluely.com.evil.test/x",
    ] {
        assert!(
            validate(url, RequestPurpose::UserConfiguredAiProvider).is_ok(),
            "{url} is a different domain and must not be blocked"
        );
    }
}

#[test]
fn case_and_trailing_dot_do_not_evade_the_blocklist() {
    for url in [
        "https://API.OpenWhispr.COM/v1",
        "https://Auth.OPENWHISPR.com/session",
        // domain-scanner:allow-next-line a negative test must name the host it asserts is blocked
        "https://openwhispr.com./v1",
        // domain-scanner:allow-next-line a negative test must name the host it asserts is blocked
        "https://API.OpenWhispr.COM./v1",
        // domain-scanner:allow-next-line a negative test must name the host it asserts is blocked
        "https://mcp.openwhispr.com./stream",
        // More than one trailing dot is not valid DNS syntax, but the strip
        // has to remove the whole run rather than exactly one -- a single
        // `strip_suffix` would leave one dot behind and miss this.
        // domain-scanner:allow-next-line a negative test must name the host it asserts is blocked
        "https://openwhispr.com../v1",
    ] {
        assert!(
            matches!(
                validate(url, RequestPurpose::UserConfiguredAiProvider),
                Err(PolicyError::BlockedVendorHost { .. })
            ),
            "{url} must be blocked regardless of case or a trailing DNS-root dot"
        );
    }
}

#[test]
fn a_blocked_host_is_still_blocked_on_loopback_plaintext() {
    // The self-hosted purpose is the only one allowed plain HTTP. That exemption
    // must not become a hole in the blocklist.
    assert!(matches!(
        validate(
            "http://openwhispr.com/v1",
            RequestPurpose::ExplicitlyConfiguredSelfHostedEndpoint
        ),
        Err(PolicyError::BlockedVendorHost { .. })
    ));
}
