//! Endpoint URL handling, ported from upstream's `constants.ts` and `urlUtils.ts`.
//!
//! One deliberate behavioural change. Upstream's `buildApiUrl` is
//! `normalizeBaseUrl(base) || "https://api.openai.com/v1"` — a second-order
//! default that silently sends a request to OpenAI whenever the configured base
//! URL is empty or unparseable. The brief forbids that, and rightly: the user
//! configured *something else*, and the failure mode is their audio and their
//! credential going to a provider they did not choose. [`build_api_url`] returns
//! an error instead.

use url::Url;

/// Suffixes stripped when normalising a base URL, in order.
///
/// Users paste whatever their provider's documentation shows them, which is
/// often a full endpoint rather than a base.
const SUFFIXES: [&str; 6] = [
    "/audio/transcriptions",
    "/audio/translations",
    "/chat/completions",
    "/completions",
    "/embeddings",
    "/models",
];

/// Reasons a URL cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UrlError {
    #[error("the endpoint URL is empty")]
    Empty,

    #[error("{url:?} is not a valid URL")]
    Unparseable { url: String },

    #[error(
        "{url:?} uses plaintext HTTP to a public host. Audio and a credential would cross \
         the network unencrypted; use HTTPS, or a private address if this is a machine on \
         your own network."
    )]
    Insecure { url: String },
}

/// Strip a known API path suffix and any trailing slash, keeping query and
/// fragment.
///
/// Returns `None` for an empty or unparseable input rather than an empty string,
/// so a caller cannot accidentally treat "nothing" as a usable base.
#[must_use]
pub fn normalize_base_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parsed = Url::parse(trimmed).ok()?;

    let mut path = parsed.path().trim_end_matches('/').to_owned();
    for suffix in SUFFIXES {
        if let Some(stripped) = path
            .to_lowercase()
            .strip_suffix(suffix)
            .map(|s| s.len())
            .map(|len| path.get(..len).unwrap_or("").to_owned())
        {
            path = stripped.trim_end_matches('/').to_owned();
            break;
        }
    }

    let origin = parsed.origin().ascii_serialization();
    if origin == "null" {
        // A non-special scheme (data:, file:) has no meaningful origin, so it is
        // not an API base.
        return None;
    }

    let mut rebuilt = format!("{origin}{path}");
    if let Some(query) = parsed.query() {
        rebuilt.push('?');
        rebuilt.push_str(query);
    }
    if let Some(fragment) = parsed.fragment() {
        rebuilt.push('#');
        rebuilt.push_str(fragment);
    }

    Some(rebuilt)
}

/// Whether a host is on a private network.
///
/// Delegates to `kw_network_policy`, deliberately. This predicate decides
/// whether plaintext HTTP is acceptable, so it is a security rule -- and route
/// resolution and the network boundary must not be able to disagree about it.
/// They previously did: this layer accepted the whole private range while the
/// policy layer accepted only loopback, so a self-hosted endpoint the settings
/// screen accepted was refused at send time.
#[must_use]
pub fn is_private_host(host: &str) -> bool {
    kw_network_policy::is_private_host(host)
}

/// Whether `url` may carry audio and a credential.
///
/// HTTPS always; plaintext HTTP only to a private host.
#[must_use]
pub fn is_secure_http_endpoint(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    match parsed.scheme() {
        "https" => true,
        "http" => parsed.host_str().is_some_and(is_private_host),
        _ => false,
    }
}

/// Host suffixes that identify an Azure OpenAI resource.
const AZURE_HOST_SUFFIXES: [&str; 3] = [
    ".openai.azure.com",
    ".cognitiveservices.azure.com",
    ".services.ai.azure.com",
];

/// The API version that supports the `gpt-4o-transcribe` family while remaining
/// compatible with `whisper-1`. Used when the user has not pinned their own.
pub const DEFAULT_AZURE_TRANSCRIPTION_API_VERSION: &str = "2025-03-01-preview";

#[must_use]
pub fn is_azure_openai_endpoint(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_lowercase))
        .is_some_and(|host| {
            AZURE_HOST_SUFFIXES
                .iter()
                .any(|suffix| host.ends_with(suffix))
        })
}

/// Build the transcription URL Azure OpenAI actually expects.
///
/// Azure does not serve the plain `{base}/audio/transcriptions` shape: it routes
/// by *deployment name* in the path and requires an `api-version` query. Without
/// this the request comes back `DeploymentNotFound`, which reads like a
/// configuration mistake rather than a URL-shape mistake.
///
/// Takes the raw URL rather than a normalised base, because normalisation strips
/// exactly the suffix that marks a URL the user pinned deliberately.
#[must_use]
pub fn build_azure_transcription_url(raw: &str, deployment: &str) -> Option<String> {
    let parsed = Url::parse(raw.trim()).ok()?;
    let path = parsed.path().trim_end_matches('/').to_lowercase();

    // The user pasted a full endpoint; respect it so they can pin a path or an
    // api-version of their own.
    if path.ends_with("/audio/transcriptions") || path.ends_with("/audio/translations") {
        return Some(parsed.to_string());
    }

    let deployment = deployment.trim();
    if deployment.is_empty() {
        // Azure has no default deployment, so there is nothing to guess.
        return None;
    }

    let version = parsed
        .query_pairs()
        .find(|(k, _)| k == "api-version")
        .map(|(_, v)| v.into_owned())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_AZURE_TRANSCRIPTION_API_VERSION.to_owned());

    let origin = parsed.origin().ascii_serialization();
    if origin == "null" {
        return None;
    }

    let encoded: String = url::form_urlencoded::byte_serialize(deployment.as_bytes()).collect();
    Some(format!(
        "{origin}/openai/deployments/{encoded}/audio/transcriptions?api-version={version}"
    ))
}

/// Join a base URL and a path.
///
/// Errors rather than defaulting. See the module documentation.
pub fn build_api_url(base: &str, path: &str) -> Result<String, UrlError> {
    if base.trim().is_empty() {
        return Err(UrlError::Empty);
    }

    let normalized = normalize_base_url(base).ok_or_else(|| UrlError::Unparseable {
        url: base.to_owned(),
    })?;

    if path.is_empty() {
        return Ok(normalized);
    }

    // Split any query or fragment off the base so the path lands before them.
    let (head, decorators) = match normalized.find(['?', '#']) {
        Some(i) => (
            normalized.get(..i).unwrap_or("").to_owned(),
            normalized.get(i..).unwrap_or("").to_owned(),
        ),
        None => (normalized.clone(), String::new()),
    };

    let path = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    };

    Ok(format!("{head}{path}{decorators}"))
}

/// Validate an endpoint the user typed, with the same rules the request path
/// will apply.
pub fn validate_endpoint(raw: &str) -> Result<String, UrlError> {
    let base = normalize_base_url(raw).ok_or_else(|| {
        if raw.trim().is_empty() {
            UrlError::Empty
        } else {
            UrlError::Unparseable {
                url: raw.to_owned(),
            }
        }
    })?;

    if !is_secure_http_endpoint(&base) {
        return Err(UrlError::Insecure { url: base });
    }

    Ok(base)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    /* ------------------------------------------------------------- normalising */

    #[test]
    fn strips_the_api_suffix_users_paste() {
        for (input, expected) in [
            (
                "https://api.openai.com/v1/audio/transcriptions",
                "https://api.openai.com/v1",
            ),
            (
                "https://api.groq.com/openai/v1/chat/completions",
                "https://api.groq.com/openai/v1",
            ),
            (
                "https://api.berget.ai/v1/models",
                "https://api.berget.ai/v1",
            ),
            ("https://api.openai.com/v1/", "https://api.openai.com/v1"),
            ("https://api.openai.com/v1", "https://api.openai.com/v1"),
        ] {
            assert_eq!(
                normalize_base_url(input).as_deref(),
                Some(expected),
                "for {input}"
            );
        }
    }

    #[test]
    fn keeps_query_and_fragment() {
        assert_eq!(
            normalize_base_url("https://x.openai.azure.com/openai?api-version=2024-01-01")
                .as_deref(),
            Some("https://x.openai.azure.com/openai?api-version=2024-01-01")
        );
    }

    #[test]
    fn nothing_normalises_to_nothing_rather_than_an_empty_string() {
        // The property that makes the fail-closed default possible.
        for input in ["", "   ", "not a url", "/relative/path"] {
            assert_eq!(normalize_base_url(input), None, "for {input:?}");
        }
    }

    /* ---------------------------------------------------------------- security */

    #[test]
    fn https_is_always_acceptable() {
        assert!(is_secure_http_endpoint("https://api.openai.com/v1"));
        assert!(is_secure_http_endpoint("https://192.168.1.5:8080/v1"));
    }

    #[test]
    fn plaintext_is_acceptable_only_to_a_private_host() {
        // This is how a self-hosted whisper server on the LAN is reached.
        for ok in [
            "http://127.0.0.1:8178/v1",
            "http://localhost:8178/v1",
            "http://192.168.1.20:9000/v1",
            "http://10.0.0.5/v1",
            "http://172.16.4.4/v1",
            "http://whisper-box/v1",
            "http://nas.local/v1",
            "http://[::1]:8178/v1",
        ] {
            assert!(is_secure_http_endpoint(ok), "{ok} should be allowed");
        }

        for bad in [
            "http://api.openai.com/v1",
            "http://example.com/v1",
            "http://8.8.8.8/v1",
            "http://172.32.0.1/v1",
        ] {
            assert!(!is_secure_http_endpoint(bad), "{bad} should be rejected");
        }
    }

    #[test]
    fn non_http_schemes_are_rejected() {
        for bad in [
            "file:///etc/passwd",
            "ftp://example.com",
            "data:text/plain,x",
            "ws://x.com",
        ] {
            assert!(!is_secure_http_endpoint(bad), "{bad}");
        }
    }

    #[test]
    fn a_public_ip_just_outside_the_private_range_is_not_private() {
        // 172.16.0.0/12 ends at 172.31.255.255.
        assert!(is_private_host("172.31.255.255"));
        assert!(!is_private_host("172.32.0.0"));
        assert!(!is_private_host("11.0.0.1"));
        assert!(!is_private_host("192.169.0.1"));
    }

    /* ------------------------------------------------------------------- azure */

    #[test]
    fn recognises_the_three_azure_host_families() {
        for host in [
            "https://mine.openai.azure.com/openai",
            "https://mine.cognitiveservices.azure.com/",
            "https://mine.services.ai.azure.com/",
        ] {
            assert!(is_azure_openai_endpoint(host), "{host}");
        }
        assert!(!is_azure_openai_endpoint("https://api.openai.com/v1"));
        // A lookalike host must not match.
        assert!(!is_azure_openai_endpoint(
            "https://openai.azure.com.evil.example/"
        ));
    }

    #[test]
    fn builds_a_deployment_url_with_the_default_api_version() {
        let built = build_azure_transcription_url("https://mine.openai.azure.com/", "my-whisper")
            .expect("should build");
        assert_eq!(
            built,
            format!(
                "https://mine.openai.azure.com/openai/deployments/my-whisper/audio/transcriptions?api-version={DEFAULT_AZURE_TRANSCRIPTION_API_VERSION}"
            )
        );
    }

    #[test]
    fn a_pinned_api_version_in_the_url_wins() {
        let built = build_azure_transcription_url(
            "https://mine.openai.azure.com/openai?api-version=2024-06-01",
            "my-whisper",
        )
        .unwrap();
        assert!(built.contains("api-version=2024-06-01"));
        assert!(!built.contains(DEFAULT_AZURE_TRANSCRIPTION_API_VERSION));
    }

    #[test]
    fn a_full_endpoint_the_user_pasted_is_respected_as_is() {
        let raw = "https://mine.openai.azure.com/openai/deployments/x/audio/transcriptions?api-version=2024-06-01";
        assert_eq!(
            build_azure_transcription_url(raw, "ignored").as_deref(),
            Some(raw)
        );
    }

    #[test]
    fn azure_without_a_deployment_name_cannot_be_built() {
        // Azure has no default deployment, so guessing would produce a request
        // that fails confusingly.
        assert_eq!(
            build_azure_transcription_url("https://mine.openai.azure.com/", ""),
            None
        );
        assert_eq!(
            build_azure_transcription_url("https://mine.openai.azure.com/", "  "),
            None
        );
    }

    #[test]
    fn a_deployment_name_needing_escaping_is_escaped() {
        let built =
            build_azure_transcription_url("https://mine.openai.azure.com/", "my whisper").unwrap();
        assert!(
            built.contains("my+whisper") || built.contains("my%20whisper"),
            "{built}"
        );
    }

    /* ------------------------------------------------------- the fail-closed bit */

    #[test]
    fn build_api_url_joins_a_configured_base() {
        assert_eq!(
            build_api_url("https://api.openai.com/v1", "/audio/transcriptions").unwrap(),
            "https://api.openai.com/v1/audio/transcriptions"
        );
        assert_eq!(
            build_api_url("https://api.berget.ai/v1/", "audio/transcriptions").unwrap(),
            "https://api.berget.ai/v1/audio/transcriptions"
        );
    }

    #[test]
    fn build_api_url_refuses_to_default_to_openai() {
        // Upstream returns https://api.openai.com/v1 here, which sends the
        // user's audio and credential to a provider they did not choose.
        for base in ["", "   ", "not a url"] {
            let err = build_api_url(base, "/audio/transcriptions").unwrap_err();
            assert!(
                !format!("{err}").contains("openai.com"),
                "the error must not name a substituted provider"
            );
        }
        assert_eq!(build_api_url("", "/x").unwrap_err(), UrlError::Empty);
    }

    #[test]
    fn build_api_url_puts_the_path_before_the_query() {
        assert_eq!(
            build_api_url("https://host/openai?api-version=1", "/audio/transcriptions").unwrap(),
            "https://host/openai/audio/transcriptions?api-version=1"
        );
    }

    #[test]
    fn validate_endpoint_distinguishes_empty_unparseable_and_insecure() {
        assert_eq!(validate_endpoint("").unwrap_err(), UrlError::Empty);
        assert!(matches!(
            validate_endpoint("nonsense").unwrap_err(),
            UrlError::Unparseable { .. }
        ));
        assert!(matches!(
            validate_endpoint("http://api.openai.com/v1").unwrap_err(),
            UrlError::Insecure { .. }
        ));
        assert_eq!(
            validate_endpoint("https://api.openai.com/v1/audio/transcriptions").unwrap(),
            "https://api.openai.com/v1"
        );
    }
}
