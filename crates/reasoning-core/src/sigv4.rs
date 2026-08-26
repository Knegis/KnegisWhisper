//! AWS Signature Version 4, as a pure function.
//!
//! Bedrock is the only thing in this project that needs it, and the AWS SDK is
//! not the way to get it. `aws-sdk-bedrockruntime` brings its own HTTP stack,
//! which would put a second client beside `kw_network_policy` and route requests
//! around the one place that validates a destination; and it brings a credential
//! *chain* that reads environment variables, `~/.aws/credentials`, IMDS, and
//! container metadata endpoints — implicit network behaviour on a machine the
//! user did not ask us to look at. The brief's seven-purpose boundary and its
//! "no hidden remote fallback" rule are both incompatible with that.
//!
//! So: the algorithm, written out, with nothing implicit. Credentials are the
//! ones the user typed and nothing else is consulted.
//!
//! # Why this can be trusted without an AWS account
//!
//! AWS publishes signing test vectors — a request, its canonical form, its
//! string-to-sign, and the expected signature. The tests below run the real
//! published example from the Signature Version 4 documentation end to end. A
//! signature that matches a vector AWS generated is correct in the only sense
//! that matters, and it is checkable here with no credentials and no network.
//!
//! Reference: <https://docs.aws.amazon.com/IAM/latest/UserGuide/create-signed-request.html>

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// The algorithm identifier AWS expects in the credential scope.
const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// The credentials a signature is made from.
///
/// `session_token` is present for temporary credentials (STS, SSO, an assumed
/// role) and absent for long-lived ones. It is not optional in the sense of
/// "nice to have": a temporary credential without its token is rejected by AWS
/// with an error that does not say so.
#[derive(Clone)]
pub struct Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

impl std::fmt::Debug for Credentials {
    /// Never prints the secret.
    ///
    /// A `#[derive(Debug)]` here would put an AWS secret access key into any
    /// log line that formats a request, which the brief forbids explicitly.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"<redacted>")
            .field(
                "session_token",
                &self.session_token.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// One request to sign.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    pub method: &'a str,
    /// The path, already URI-encoded, beginning with `/`.
    pub path: &'a str,
    /// The query string without the leading `?`. Empty when there is none.
    pub query: &'a str,
    /// Headers to sign. `host` must be present; `x-amz-date` is added here.
    pub headers: Vec<(String, String)>,
    pub body: &'a [u8],
}

/// Where and when.
#[derive(Debug, Clone)]
pub struct Scope<'a> {
    pub region: &'a str,
    pub service: &'a str,
    /// `YYYYMMDDTHHMMSSZ`, in UTC. Passed in rather than read from the clock so
    /// this function is pure and the published vectors can be reproduced.
    pub timestamp: &'a str,
}

/// Lowercase hex of the SHA-256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    // `new_from_slice` only fails for a key length HMAC cannot take, and HMAC
    // accepts any length, so this cannot fail. Handled rather than unwrapped
    // because the workspace denies unwrap in library code.
    match HmacSha256::new_from_slice(key) {
        Ok(mut mac) => {
            mac.update(data.as_bytes());
            mac.finalize().into_bytes().to_vec()
        }
        Err(_) => Vec::new(),
    }
}

/// Trim and collapse internal runs of spaces, as the specification requires.
///
/// AWS folds runs of whitespace in a header value to a single space *outside of
/// quoted sections*. No header this project sends is quoted, so the simple form
/// is correct here and is what the published vectors exercise.
fn canonical_header_value(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The canonical request, and the list of signed header names.
///
/// Returned together because the two must agree exactly — a header in the
/// canonical request that is missing from `SignedHeaders` produces a signature
/// mismatch whose error message names neither.
#[must_use]
pub fn canonical_request(request: &Request<'_>, payload_hash: &str) -> (String, String) {
    let mut headers: Vec<(String, String)> = request
        .headers
        .iter()
        .map(|(name, value)| (name.to_lowercase(), canonical_header_value(value)))
        .collect();

    // Sorted by lowercase name, which is the specification's ordering.
    headers.sort_by(|a, b| a.0.cmp(&b.0));

    let canonical_headers = headers
        .iter()
        .map(|(name, value)| format!("{name}:{value}\n"))
        .collect::<String>();

    let signed_headers = headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let canonical = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        request.method,
        if request.path.is_empty() {
            "/"
        } else {
            request.path
        },
        request.query,
        canonical_headers,
        signed_headers,
        payload_hash
    );

    (canonical, signed_headers)
}

/// The string that actually gets signed.
#[must_use]
pub fn string_to_sign(scope: &Scope<'_>, canonical: &str) -> String {
    let date = date_of(scope.timestamp);
    format!(
        "{ALGORITHM}\n{}\n{date}/{}/{}/aws4_request\n{}",
        scope.timestamp,
        scope.region,
        scope.service,
        sha256_hex(canonical.as_bytes())
    )
}

/// The `YYYYMMDD` prefix of an ISO basic timestamp.
fn date_of(timestamp: &str) -> &str {
    timestamp.get(..8).unwrap_or(timestamp)
}

/// The derived signing key: four chained HMACs, one per scope component.
#[must_use]
pub fn signing_key(secret: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac(format!("AWS4{secret}").as_bytes(), date);
    let k_region = hmac(&k_date, region);
    let k_service = hmac(&k_region, service);
    hmac(&k_service, "aws4_request")
}

/// A signed request: the headers to add, ready to send.
#[derive(Debug, Clone)]
pub struct Signed {
    /// Every header the request must carry, including the ones added here.
    pub headers: Vec<(String, String)>,
}

/// Sign a request.
///
/// Adds `x-amz-date`, `x-amz-security-token` where applicable, and
/// `Authorization`. The caller sends exactly [`Signed::headers`] — adding one
/// afterwards invalidates the signature, which is why this returns the whole
/// set rather than the additions.
///
/// **No `x-amz-content-sha256`.** S3 requires it; Bedrock does not, and the
/// payload hash is already the last line of the canonical request, so the body
/// is covered either way. Leaving it out is what lets `sign` itself be checked
/// against AWS's published vector rather than only its internals — and a
/// signing routine nobody can check against an external answer is one you have
/// to take on trust.
#[must_use]
pub fn sign(request: &Request<'_>, scope: &Scope<'_>, credentials: &Credentials) -> Signed {
    let payload_hash = sha256_hex(request.body);

    let mut headers = request.headers.clone();
    headers.push(("x-amz-date".to_owned(), scope.timestamp.to_owned()));
    if let Some(token) = &credentials.session_token {
        // Signed, not merely sent. AWS rejects a temporary credential whose
        // token was excluded from the signature.
        headers.push(("x-amz-security-token".to_owned(), token.clone()));
    }

    let signing_request = Request {
        headers: headers.clone(),
        ..request.clone()
    };

    let (canonical, signed_headers) = canonical_request(&signing_request, &payload_hash);
    let to_sign = string_to_sign(scope, &canonical);

    let date = date_of(scope.timestamp);
    let key = signing_key(
        &credentials.secret_access_key,
        date,
        scope.region,
        scope.service,
    );
    let signature = hex(&hmac(&key, &to_sign));

    headers.push((
        "authorization".to_owned(),
        format!(
            "{ALGORITHM} Credential={}/{date}/{}/{}/aws4_request, SignedHeaders={signed_headers}, Signature={signature}",
            credentials.access_key_id, scope.region, scope.service
        ),
    ));

    Signed { headers }
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

    /// AWS's own published example, from the Signature Version 4 documentation.
    ///
    /// A `GET` against `iam.amazonaws.com` with a fixed date and the example
    /// credentials. Every intermediate value below is copied from that page, so
    /// this asserts our implementation against AWS rather than against itself.
    fn aws_example() -> (Request<'static>, Scope<'static>, Credentials) {
        (
            Request {
                method: "GET",
                path: "/",
                query: "Action=ListUsers&Version=2010-05-08",
                headers: vec![
                    (
                        "content-type".to_owned(),
                        "application/x-www-form-urlencoded; charset=utf-8".to_owned(),
                    ),
                    ("host".to_owned(), "iam.amazonaws.com".to_owned()),
                    ("x-amz-date".to_owned(), "20150830T123600Z".to_owned()),
                ],
                body: b"",
            },
            Scope {
                region: "us-east-1",
                service: "iam",
                timestamp: "20150830T123600Z",
            },
            Credentials {
                access_key_id: "AKIDEXAMPLE".to_owned(),
                secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_owned(),
                session_token: None,
            },
        )
    }

    /// The canonical request matches AWS's published one, byte for byte.
    #[test]
    fn the_canonical_request_matches_the_published_vector() {
        let (request, _, _) = aws_example();
        let payload_hash = sha256_hex(b"");
        let (canonical, signed_headers) = canonical_request(&request, &payload_hash);

        assert_eq!(
            canonical,
            "GET\n\
             /\n\
             Action=ListUsers&Version=2010-05-08\n\
             content-type:application/x-www-form-urlencoded; charset=utf-8\n\
             host:iam.amazonaws.com\n\
             x-amz-date:20150830T123600Z\n\
             \n\
             content-type;host;x-amz-date\n\
             e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(signed_headers, "content-type;host;x-amz-date");
    }

    /// The empty-payload hash is the well-known SHA-256 of nothing.
    #[test]
    fn the_empty_payload_hash_is_the_documented_constant() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    /// The string-to-sign matches AWS's published one.
    #[test]
    fn the_string_to_sign_matches_the_published_vector() {
        let (request, scope, _) = aws_example();
        let (canonical, _) = canonical_request(&request, &sha256_hex(b""));

        assert_eq!(
            string_to_sign(&scope, &canonical),
            "AWS4-HMAC-SHA256\n\
             20150830T123600Z\n\
             20150830/us-east-1/iam/aws4_request\n\
             f536975d06c0309214f805bb90ccff089219ecd68b2577efef23edd43b7e1a59"
        );
    }

    /// The derived signing key matches AWS's published one.
    #[test]
    fn the_signing_key_matches_the_published_vector() {
        let (_, scope, credentials) = aws_example();
        let key = signing_key(
            &credentials.secret_access_key,
            "20150830",
            scope.region,
            scope.service,
        );
        assert_eq!(
            hex(&key),
            "c4afb1cc5771d871763a393e44b703571b55cc28424d1a5e86da6ed3c154a4b9"
        );
    }

    /// And the signature itself. This is the end-to-end check.
    #[test]
    fn the_signature_matches_the_published_vector() {
        let (request, scope, credentials) = aws_example();
        let (canonical, _) = canonical_request(&request, &sha256_hex(b""));
        let to_sign = string_to_sign(&scope, &canonical);
        let key = signing_key(
            &credentials.secret_access_key,
            "20150830",
            scope.region,
            scope.service,
        );

        assert_eq!(
            hex(&hmac(&key, &to_sign)),
            "5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
        );
    }

    /// `sign` produces the same signature through the public entry point.
    ///
    /// This is the test that matters: not that the pieces match AWS, but that
    /// the function callers actually use does. It is also why `sign` adds no
    /// header the vector lacks — an extra signed header would be defensible and
    /// would make this check impossible.
    ///
    /// The vector's request already carries `x-amz-date`, and `sign` adds one,
    /// so it is removed first. That pins the rule too: a caller must not pre-set
    /// the headers `sign` owns.
    #[test]
    fn signing_a_request_without_a_preset_date_reproduces_the_vector() {
        let (mut request, scope, credentials) = aws_example();
        request.headers.retain(|(name, _)| name != "x-amz-date");

        let signed = sign(&request, &scope, &credentials);
        let authorization = signed
            .headers
            .iter()
            .find(|(name, _)| name == "authorization")
            .map(|(_, v)| v.clone())
            .expect("an Authorization header");

        assert!(
            authorization.contains(
                "Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
            ),
            "{authorization}"
        );
        assert!(
            authorization.starts_with(
                "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request"
            ),
            "{authorization}"
        );
    }

    /// A session token is signed, not merely attached.
    ///
    /// Attaching it without signing it is the failure mode that looks like it
    /// works with long-lived credentials and fails only for the temporary ones
    /// most organisations actually issue.
    #[test]
    fn a_session_token_is_part_of_the_signature() {
        let (request, scope, mut credentials) = aws_example();

        let without = sign(&request, &scope, &credentials);
        credentials.session_token = Some("a-temporary-token".to_owned());
        let with = sign(&request, &scope, &credentials);

        let auth = |s: &Signed| {
            s.headers
                .iter()
                .find(|(n, _)| n == "authorization")
                .map(|(_, v)| v.clone())
                .unwrap()
        };

        assert_ne!(auth(&without), auth(&with), "the token changed nothing");
        assert!(
            auth(&with).contains("x-amz-security-token"),
            "the token must be in SignedHeaders: {}",
            auth(&with)
        );
        assert!(with
            .headers
            .iter()
            .any(|(n, _)| n == "x-amz-security-token"));
    }

    /// A body changes the signature, because the payload hash is signed.
    #[test]
    fn the_body_is_covered_by_the_signature() {
        let (request, scope, credentials) = aws_example();
        let with_body = Request {
            body: br#"{"messages":[]}"#,
            ..request.clone()
        };

        let auth = |r: &Request<'_>| {
            sign(r, &scope, &credentials)
                .headers
                .iter()
                .find(|(n, _)| n == "authorization")
                .map(|(_, v)| v.clone())
                .unwrap()
        };

        assert_ne!(auth(&request), auth(&with_body));
    }

    /// Header values are folded, and names are compared lowercase.
    #[test]
    fn header_names_and_values_are_canonicalised() {
        let request = Request {
            method: "POST",
            path: "/",
            query: "",
            headers: vec![
                ("Host".to_owned(), "example.com".to_owned()),
                ("X-Custom".to_owned(), "  a   b  ".to_owned()),
            ],
            body: b"",
        };
        let (canonical, signed) = canonical_request(&request, &sha256_hex(b""));

        assert!(canonical.contains("host:example.com\n"), "{canonical}");
        assert!(canonical.contains("x-custom:a b\n"), "{canonical}");
        // Sorted, lowercase, semicolon-separated.
        assert_eq!(signed, "host;x-custom");
    }

    /// Debug output never carries a secret.
    #[test]
    fn credentials_do_not_print_themselves() {
        let credentials = Credentials {
            access_key_id: "AKIDEXAMPLE".to_owned(),
            secret_access_key: "a-real-secret".to_owned(),
            session_token: Some("a-real-token".to_owned()),
        };
        let printed = format!("{credentials:?}");
        assert!(!printed.contains("a-real-secret"), "{printed}");
        assert!(!printed.contains("a-real-token"), "{printed}");
        // The key id is not a secret and is useful in a log line.
        assert!(printed.contains("AKIDEXAMPLE"), "{printed}");
    }
}
