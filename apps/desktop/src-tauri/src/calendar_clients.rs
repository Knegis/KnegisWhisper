//! Resolve desktop OAuth clients entirely in Rust. Build values are public
//! desktop credentials; only presence and source cross IPC, never a secret.
use kw_calendar_core::{CalendarProviderKind, OAuthClientConfig};
use kw_security::Secret;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ClientSource {
    App,
    Custom,
    Missing,
}

#[derive(Debug, Clone)]
pub enum ClientChoice {
    Custom(OAuthClientConfig),
    AppOwned(OAuthClientConfig),
    Missing,
}

impl ClientChoice {
    pub const fn source(&self) -> ClientSource {
        match self {
            Self::Custom(_) => ClientSource::Custom,
            Self::AppOwned(_) => ClientSource::App,
            Self::Missing => ClientSource::Missing,
        }
    }
}

struct Defaults<'a> {
    google_id: Option<&'a str>,
    google_secret: Option<&'a str>,
    microsoft_id: Option<&'a str>,
}

/// Compile-time values deliberately have no frontend/Vite equivalent.
pub fn resolve(
    provider: CalendarProviderKind,
    custom_id: Option<&str>,
    custom_secret: Option<Secret>,
) -> ClientChoice {
    resolve_with(
        Defaults {
            google_id: option_env!("KW_GOOGLE_CALENDAR_CLIENT_ID"),
            google_secret: option_env!("KW_GOOGLE_CALENDAR_CLIENT_SECRET"),
            microsoft_id: option_env!("KW_MICROSOFT_CALENDAR_CLIENT_ID"),
        },
        provider,
        custom_id,
        custom_secret,
    )
}

fn nonblank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

fn resolve_with(
    defaults: Defaults<'_>,
    provider: CalendarProviderKind,
    custom_id: Option<&str>,
    custom_secret: Option<Secret>,
) -> ClientChoice {
    if let Some(id) = nonblank(custom_id) {
        return ClientChoice::Custom(match provider {
            CalendarProviderKind::Google => OAuthClientConfig::Google {
                client_id: id.to_owned(),
                client_secret: custom_secret,
            },
            CalendarProviderKind::Microsoft => OAuthClientConfig::Microsoft {
                client_id: id.to_owned(),
            },
        });
    }
    match provider {
        CalendarProviderKind::Google => match (
            nonblank(defaults.google_id),
            nonblank(defaults.google_secret),
        ) {
            (Some(id), Some(secret)) => ClientChoice::AppOwned(OAuthClientConfig::Google {
                client_id: id.to_owned(),
                client_secret: Some(Secret::new(secret.to_owned())),
            }),
            _ => ClientChoice::Missing,
        },
        CalendarProviderKind::Microsoft => match nonblank(defaults.microsoft_id) {
            Some(id) => ClientChoice::AppOwned(OAuthClientConfig::Microsoft {
                client_id: id.to_owned(),
            }),
            None => ClientChoice::Missing,
        },
    }
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

    #[test]
    fn calendar_client_resolution_table() {
        for (google_id, google_secret, microsoft_id) in [
            (None, None, None),
            (Some(" "), Some("secret"), Some(" ")),
            (Some("app-google"), None, Some("app-ms")),
            (None, Some("secret"), Some("app-ms")),
            (Some("app-google"), Some(" "), None),
            (Some(" app-google "), Some(" app-secret "), Some(" app-ms ")),
        ] {
            for provider in [
                CalendarProviderKind::Google,
                CalendarProviderKind::Microsoft,
            ] {
                for custom_id in [None, Some(" "), Some(" custom ")] {
                    for custom_secret in [None, Some(Secret::new("custom-secret".into()))] {
                        let choice = resolve_with(
                            Defaults {
                                google_id,
                                google_secret,
                                microsoft_id,
                            },
                            provider,
                            custom_id,
                            custom_secret.clone(),
                        );
                        if nonblank(custom_id).is_some() {
                            let ClientChoice::Custom(client) = choice else {
                                panic!("custom must win")
                            };
                            assert_eq!(client.client_id(), "custom");
                            assert_eq!(client.kind(), provider);
                            assert_eq!(
                                client.client_secret().map(Secret::expose),
                                if provider == CalendarProviderKind::Google {
                                    custom_secret.as_ref().map(Secret::expose)
                                } else {
                                    None
                                }
                            );
                        } else {
                            let available = match provider {
                                CalendarProviderKind::Google => {
                                    nonblank(google_id).is_some()
                                        && nonblank(google_secret).is_some()
                                }
                                CalendarProviderKind::Microsoft => nonblank(microsoft_id).is_some(),
                            };
                            assert_eq!(
                                choice.source(),
                                if available {
                                    ClientSource::App
                                } else {
                                    ClientSource::Missing
                                }
                            );
                            if let ClientChoice::AppOwned(client) = choice {
                                assert_eq!(
                                    client.client_id(),
                                    if provider == CalendarProviderKind::Google {
                                        "app-google"
                                    } else {
                                        "app-ms"
                                    }
                                );
                                assert_eq!(
                                    client.client_secret().map(Secret::expose),
                                    if provider == CalendarProviderKind::Google {
                                        Some("app-secret")
                                    } else {
                                        None
                                    }
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
