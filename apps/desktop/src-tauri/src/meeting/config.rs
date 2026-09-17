//! Deciding what to connect with, before anything connects.
//!
//! Lifted out of `meeting.rs` on 2026-09-01 (KNEG-89). Nothing changed in the
//! move — same functions, same tests, same names.
//!
//! Every function here is free rather than a [`super::MeetingRuntime`] method,
//! and that is the property worth keeping: each takes plain arguments and a
//! `&dyn CredentialStore`, so the whole of "may this meeting start, and with
//! what" is exercised against a `MemoryCredentialStore` with no `AppHandle`
//! anywhere. `media.rs:1334-1340` records why that matters — nothing outside a
//! running application can construct one, so logic reachable only through one
//! is untestable by construction.
//!
//! It is also where a real defect was fixed and is now pinned by tests:
//! `start` used to build its `MeetingProviderConfig` with `key_ref: None`
//! unconditionally, so every OpenAI Realtime attempt was refused from inside
//! `connect` as a socket error rather than here as a sentence naming the
//! settings screen.

use kw_meeting_core::{Client, MeetingProviderConfig};

/// Resolve the credential reference `client` should connect with, refusing
/// with a message naming the settings screen when its slot has nothing
/// stored.
///
/// A free function taking `&dyn CredentialStore` rather than a
/// `MeetingRuntime` method, so it can be unit-tested against a
/// `MemoryCredentialStore` with no `AppHandle` in sight. It only ever calls
/// [`kw_security::CredentialStore::contains`], never `get`: this is the
/// controller deciding whether to proceed, not the moment a secret is
/// needed, so it must never hold one.
///
/// This is the fix for a real defect: `start` used to build its
/// `MeetingProviderConfig` with `key_ref: None` unconditionally, so
/// `openai::token` refused every OpenAI Realtime connection attempt no matter
/// what was stored, and the failure only ever surfaced from inside `connect`
/// -- a socket error, not this message.
pub(super) fn resolve_key_ref(
    provider_id: &str,
    client: Client,
    credentials: &dyn kw_security::CredentialStore,
) -> Result<Option<kw_security::KeyRef>, String> {
    let Some(slot) = kw_meeting_core::credential_slot(client) else {
        return Ok(None);
    };

    let key_ref = kw_security::KeyRef::new(slot)
        .ok_or_else(|| format!("the credential slot {slot:?} for this provider is not valid"))?;

    if !credentials.contains(&key_ref) {
        let name = kw_meeting_core::capabilities(provider_id).map_or(provider_id, |c| c.name);
        return Err(format!(
            "no {name} key is stored. Add one under Settings > Providers."
        ));
    }

    Ok(Some(key_ref))
}

/// Resolve `provider_id` to the [`Client`] that implements it, refusing with
/// the declined reason or a generic "no client" message when there is none.
///
/// A free function for the same reason `resolve_key_ref` is: `start` needs
/// the result before it can look up a credential slot, so pulling it out of
/// `provider` keeps that lookup from happening twice.
pub(super) fn resolve_client(provider_id: &str) -> Result<Client, String> {
    kw_meeting_core::client(provider_id).ok_or_else(|| {
        match kw_meeting_core::declined(provider_id) {
            Some(reason) => reason.to_owned(),
            None => format!("there is no meeting client for {provider_id:?}"),
        }
    })
}

/// Corti's environment and tenant, from the same settings the batch
/// transcription client reads -- reused rather than duplicated, because they
/// are properties of the Corti tenancy, not of the meeting feature. Refuses
/// by name when either is missing; see
/// [`kw_meeting_core::corti::scope`]'s doc comment for why this mirrors
/// `route.rs`'s `required` rather than defaulting.
///
/// A free function taking `&RouteSettings` rather than reading the database
/// itself, for the same reason `resolve_key_ref` and `resolve_client` are
/// free functions: the refusal can then be tested with no `AppHandle` and no
/// database in sight.
pub(super) fn resolve_corti_scope(
    settings: &kw_transcription_core::RouteSettings,
) -> Result<(String, String), String> {
    kw_meeting_core::corti::scope(
        settings.corti_environment.as_deref().unwrap_or(""),
        settings.corti_tenant.as_deref().unwrap_or(""),
    )
    .map_err(|e| e.to_string())
}

/// A language setting normalised to what [`MeetingProviderConfig`] wants: a
/// blank field becomes `None` rather than the literal empty string, so
/// [`kw_meeting_core::validate`] reads it the same way whether the setting was
/// never written or was written and cleared.
pub(super) fn normalize_language(language: &str) -> Option<String> {
    let trimmed = language.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Keyterms normalised the same way: each one trimmed, and blanks dropped so a
/// stray empty line from the editor does not count against a provider's cap.
pub(super) fn normalize_keyterms(keyterms: &[String]) -> Vec<String> {
    keyterms
        .iter()
        .map(|k| k.trim().to_owned())
        .filter(|k| !k.is_empty())
        .collect()
}

/// Build the config a settings-screen preview should see for `provider_id`,
/// from raw form values.
///
/// A free function, taking the draft values directly rather than reading them
/// from settings, so the screen can preview a value before it is saved and so
/// this can be tested with no database in sight. Whether the result needs a
/// language is [`kw_meeting_core::validate`]'s answer to give, not this
/// function's -- it only normalises and looks up the provider's declared
/// sample rate, the same way [`MeetingRuntime::start`] does.
pub(super) fn draft_config(
    provider_id: &str,
    language: &str,
    keyterms: &[String],
) -> Result<MeetingProviderConfig, String> {
    let caps = kw_meeting_core::capabilities(provider_id)
        .ok_or_else(|| format!("{provider_id:?} is not a described meeting provider"))?;
    let sample_rate = *caps.sample_rates.first().ok_or_else(|| {
        format!(
            "{} declares no sample rate, so nothing can be reviewed for it",
            caps.name
        )
    })?;

    Ok(MeetingProviderConfig {
        provider: provider_id.to_owned(),
        model: (provider_id == "deepgram-realtime" && !keyterms.is_empty())
            .then(|| "nova-3".to_owned()),
        language: normalize_language(language),
        keyterms: normalize_keyterms(keyterms),
        key_ref: None,
        sample_rate,
    })
}

/// Build the config [`MeetingTranscriptionProvider::connect`] will use,
/// refusing first if `caps` needs a language and `language` gives it none.
///
/// A free function taking the provider's capabilities and the raw settings
/// values rather than reading them itself, for the same reason
/// `resolve_key_ref` is one: `start`'s refusal can then be tested with neither
/// a database nor an `AppHandle` in sight. [`kw_meeting_core::validate`] is
/// the one place the keyterm and language arithmetic lives; this only acts on
/// what it reports, and never picks a language on its own -- a guessed
/// language is a worse transcript that looks like a working one.
pub(super) fn resolve_provider_config(
    provider_id: &str,
    caps: &kw_meeting_core::MeetingProviderCapabilities,
    key_ref: Option<kw_security::KeyRef>,
    sample_rate: u32,
    language: &str,
    keyterms: &[String],
) -> Result<MeetingProviderConfig, String> {
    let config = MeetingProviderConfig {
        provider: provider_id.to_owned(),
        model: (provider_id == "deepgram-realtime" && !keyterms.is_empty())
            .then(|| "nova-3".to_owned()),
        language: normalize_language(language),
        keyterms: normalize_keyterms(keyterms),
        key_ref,
        sample_rate,
    };

    let review = kw_meeting_core::validate(&config).map_err(|e| e.to_string())?;
    if review.language_required {
        return Err(format!(
            "{} needs a concrete language, and none is set. Add one under Settings > Meetings.",
            caps.name
        ));
    }

    Ok(config)
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

    // The settings keys and `Db` live in the parent, which owns the storage
    // side of a meeting. Named here rather than moved, because a key is read by
    // the runtime as well and one copy is one thing to keep in step.
    use super::super::{KEYTERMS_KEY, LANGUAGE_KEY};
    use kw_storage::Db;
    // Only for `resolve_key_ref`'s tests, to call `set`/`contains` on a
    // `MemoryCredentialStore` by trait method rather than a fully-qualified
    // path.
    use kw_security::CredentialStore as _;

    #[test]
    fn every_client_can_be_built_by_the_dispatch() {
        // The exhaustive match in `provider` is what makes a new client a compile
        // error until it is wired. This asserts the other half: that every
        // allowed provider has a `Client` the match covers.
        for provider in kw_meeting_core::PROVIDERS {
            let allowed = kw_meeting_core::is_allowed(provider.id);
            let client = kw_meeting_core::client(provider.id);
            assert_eq!(allowed, client.is_some(), "{}", provider.id);
        }
    }

    #[test]
    fn resolve_client_resolves_the_clients_that_exist() {
        assert_eq!(resolve_client("local"), Ok(Client::LocalWindow));
        assert_eq!(
            resolve_client("openai-realtime"),
            Ok(Client::OpenAiRealtime)
        );
        assert_eq!(
            resolve_client("assemblyai-realtime"),
            Ok(Client::AssemblyAi)
        );
        assert_eq!(
            resolve_client("deepgram-realtime"),
            Ok(Client::DeepgramRealtime)
        );
        assert_eq!(resolve_client("corti-realtime"), Ok(Client::CortiRealtime));
    }

    #[test]
    fn resolve_client_names_the_declined_reason_rather_than_a_generic_message() {
        let err = resolve_client("tinfoil-realtime").unwrap_err();
        assert!(err.contains("attestation"), "{err}");
    }

    #[test]
    fn resolve_client_refuses_an_unknown_provider_id() {
        let err = resolve_client("not-a-real-provider").unwrap_err();
        assert!(err.contains("no meeting client"), "{err}");
    }

    // `resolve_key_ref` is a free function taking `&dyn CredentialStore`
    // precisely so these can run with no `AppHandle`, no Tauri runtime, and no
    // real keychain -- see its doc comment for why holding a reference to
    // Tauri-dependent code in a test is not an option here.

    #[test]
    fn local_needs_no_credential_even_when_the_store_is_empty() {
        let store = kw_security::MemoryCredentialStore::new();
        let key_ref = resolve_key_ref("local", Client::LocalWindow, &store).unwrap();
        assert_eq!(key_ref, None);
    }

    #[test]
    fn openai_realtime_refuses_to_start_naming_the_settings_screen_when_nothing_is_stored() {
        // The tripwire for the defect this task fixes: `key_ref: None` used to
        // reach `connect` regardless, so the failure a user saw was a socket
        // error with no mention of where to fix it.
        let store = kw_security::MemoryCredentialStore::new();
        let err = resolve_key_ref("openai-realtime", Client::OpenAiRealtime, &store).unwrap_err();
        assert!(err.contains("Settings > Providers"), "{err}");
    }

    #[test]
    fn openai_realtime_gets_past_the_check_once_a_key_is_stored() {
        let store = kw_security::MemoryCredentialStore::new();
        store
            .set(
                &kw_security::KeyRef::new("openai").unwrap(),
                &kw_security::Secret::new("sk-test".to_owned()),
            )
            .unwrap();

        let key_ref = resolve_key_ref("openai-realtime", Client::OpenAiRealtime, &store).unwrap();
        assert_eq!(key_ref, kw_security::KeyRef::new("openai"));
    }

    #[test]
    fn a_blank_stored_credential_is_treated_as_absent() {
        // `CredentialStore::contains` already treats whitespace-only as empty;
        // this asserts the controller's refusal actually depends on that, not
        // on some looser check of its own.
        let store = kw_security::MemoryCredentialStore::new();
        store
            .set(
                &kw_security::KeyRef::new("openai").unwrap(),
                &kw_security::Secret::new("   ".to_owned()),
            )
            .unwrap();

        let err = resolve_key_ref("openai-realtime", Client::OpenAiRealtime, &store).unwrap_err();
        assert!(err.contains("Settings > Providers"), "{err}");
    }

    #[test]
    fn assemblyai_realtime_refuses_to_start_naming_the_settings_screen_when_nothing_is_stored() {
        let store = kw_security::MemoryCredentialStore::new();
        let err = resolve_key_ref("assemblyai-realtime", Client::AssemblyAi, &store).unwrap_err();
        assert!(err.contains("Settings > Providers"), "{err}");
    }

    #[test]
    fn assemblyai_realtime_gets_past_the_check_once_a_key_is_stored() {
        let store = kw_security::MemoryCredentialStore::new();
        store
            .set(
                &kw_security::KeyRef::new("assemblyai").unwrap(),
                &kw_security::Secret::new("aai-test".to_owned()),
            )
            .unwrap();

        let key_ref = resolve_key_ref("assemblyai-realtime", Client::AssemblyAi, &store).unwrap();
        assert_eq!(key_ref, kw_security::KeyRef::new("assemblyai"));
    }

    #[test]
    fn deepgram_realtime_refuses_to_start_naming_the_settings_screen_when_nothing_is_stored() {
        let store = kw_security::MemoryCredentialStore::new();
        let err =
            resolve_key_ref("deepgram-realtime", Client::DeepgramRealtime, &store).unwrap_err();
        assert!(err.contains("Settings > Providers"), "{err}");
    }

    #[test]
    fn deepgram_realtime_gets_past_the_check_once_a_key_is_stored() {
        let store = kw_security::MemoryCredentialStore::new();
        store
            .set(
                &kw_security::KeyRef::new("deepgram").unwrap(),
                &kw_security::Secret::new("dg-test".to_owned()),
            )
            .unwrap();

        let key_ref =
            resolve_key_ref("deepgram-realtime", Client::DeepgramRealtime, &store).unwrap();
        assert_eq!(key_ref, kw_security::KeyRef::new("deepgram"));
    }

    #[test]
    fn corti_realtime_refuses_to_start_naming_the_settings_screen_when_nothing_is_stored() {
        let store = kw_security::MemoryCredentialStore::new();
        let err = resolve_key_ref("corti-realtime", Client::CortiRealtime, &store).unwrap_err();
        assert!(err.contains("Settings > Providers"), "{err}");
    }

    #[test]
    fn corti_realtime_gets_past_the_check_once_a_key_is_stored() {
        let store = kw_security::MemoryCredentialStore::new();
        store
            .set(
                &kw_security::KeyRef::new("corti").unwrap(),
                &kw_security::Secret::new("corti-test".to_owned()),
            )
            .unwrap();

        let key_ref = resolve_key_ref("corti-realtime", Client::CortiRealtime, &store).unwrap();
        assert_eq!(key_ref, kw_security::KeyRef::new("corti"));
    }

    #[test]
    fn corti_realtime_refuses_to_build_naming_which_of_environment_or_tenant_is_missing() {
        // `resolve_corti_scope` is what `provider`'s `Client::CortiRealtime`
        // arm calls before it ever constructs a socket -- this is the same
        // refusal `route.rs::required` fires for batch transcription, reached
        // from a second caller rather than duplicated.
        let settings = kw_transcription_core::RouteSettings::default();
        let err = resolve_corti_scope(&settings).unwrap_err();
        assert!(err.contains("environment"), "{err}");

        let settings = kw_transcription_core::RouteSettings {
            corti_environment: Some("eu".to_owned()),
            ..Default::default()
        };
        let err = resolve_corti_scope(&settings).unwrap_err();
        assert!(err.contains("tenant"), "{err}");
    }

    #[test]
    fn corti_realtime_scope_resolves_once_both_are_set() {
        let settings = kw_transcription_core::RouteSettings {
            corti_environment: Some(" eu ".to_owned()),
            corti_tenant: Some(" acme ".to_owned()),
            ..Default::default()
        };
        let (environment, tenant) = resolve_corti_scope(&settings).unwrap();
        assert_eq!(environment, "eu");
        assert_eq!(tenant, "acme");
    }

    #[test]
    fn every_clients_credential_slot_is_a_valid_key_ref() {
        // `resolve_key_ref`'s `KeyRef::new(slot).ok_or_else(...)` branch stayed
        // unreachable and untested while only two `Client` variants existed --
        // both slots happened to be valid strings, so nothing ever exercised
        // the refusal. A third arm is exactly where a typo producing an
        // invalid slot name (stray punctuation, a capital letter) would first
        // show up, so this checks every arm directly against the validation
        // `resolve_key_ref` depends on, rather than trusting that the slots
        // that happen to exist today stay well-formed by accident.
        for client in [
            Client::LocalWindow,
            Client::OpenAiRealtime,
            Client::AssemblyAi,
            Client::DeepgramRealtime,
            Client::CortiRealtime,
        ] {
            if let Some(slot) = kw_meeting_core::credential_slot(client) {
                assert!(
                    kw_security::KeyRef::new(slot).is_some(),
                    "{slot:?} is not a valid credential slot name"
                );
            }
        }
    }

    // `resolve_provider_config`, `draft_config` and the settings keys are all
    // free functions and a database, precisely so the properties below can be
    // asserted with no `AppHandle` in sight -- see their doc comments.

    #[tokio::test]
    async fn the_language_and_keyterms_settings_round_trip() {
        let db = Db::open_in_memory().await.unwrap();
        let pool = db.pool();

        // Nothing stored yet: language is unset and there are no keyterms.
        assert_eq!(
            kw_storage::settings::get_or(pool, LANGUAGE_KEY, String::new())
                .await
                .unwrap(),
            ""
        );
        assert!(
            kw_storage::settings::get_or(pool, KEYTERMS_KEY, Vec::<String>::new())
                .await
                .unwrap()
                .is_empty()
        );

        kw_storage::settings::set(pool, LANGUAGE_KEY, &"sv".to_owned())
            .await
            .unwrap();
        let keyterms = vec!["Sprint".to_owned(), "Q3 roadmap".to_owned()];
        kw_storage::settings::set(pool, KEYTERMS_KEY, &keyterms)
            .await
            .unwrap();

        assert_eq!(
            kw_storage::settings::get_or(pool, LANGUAGE_KEY, String::new())
                .await
                .unwrap(),
            "sv"
        );
        assert_eq!(
            kw_storage::settings::get_or(pool, KEYTERMS_KEY, Vec::<String>::new())
                .await
                .unwrap(),
            keyterms
        );
    }

    #[test]
    fn corti_refuses_to_start_naming_the_provider_and_the_field_when_no_language_is_set() {
        // The tripwire for the property this task exists to add: `start` used
        // to build its config with `language: None` unconditionally and no
        // check at all, so a provider that cannot detect a language would have
        // failed three seconds later at `connect`, or worse, produced a
        // transcript in the wrong language with nothing said about it.
        let caps = kw_meeting_core::capabilities("corti-realtime").unwrap();
        let err =
            resolve_provider_config("corti-realtime", caps, None, 16_000, "", &[]).unwrap_err();
        assert!(err.contains("Corti Realtime"), "{err}");
        assert!(err.contains("language"), "{err}");
        assert!(err.contains("Settings > Meetings"), "{err}");
    }

    #[test]
    fn corti_proceeds_once_a_language_is_given() {
        let caps = kw_meeting_core::capabilities("corti-realtime").unwrap();
        let config =
            resolve_provider_config("corti-realtime", caps, None, 16_000, "sv", &[]).unwrap();
        assert_eq!(config.language, Some("sv".to_owned()));
    }

    #[test]
    fn a_provider_that_detects_language_is_not_blocked_by_a_blank_field() {
        let caps = kw_meeting_core::capabilities("openai-realtime").unwrap();
        let config =
            resolve_provider_config("openai-realtime", caps, None, 24_000, "", &[]).unwrap();
        assert_eq!(config.language, None);
    }

    #[test]
    fn the_local_provider_is_not_blocked_by_a_blank_field_either() {
        // The regression this task's own change to `capabilities.rs` guards
        // against: the one provider that needs no credentials and no network
        // must keep working with nothing configured.
        let caps = kw_meeting_core::capabilities("local").unwrap();
        let config = resolve_provider_config("local", caps, None, 16_000, "", &[]).unwrap();
        assert_eq!(config.language, None);
    }

    #[test]
    fn a_blank_language_setting_is_not_sent_as_the_literal_empty_string() {
        assert_eq!(normalize_language("   "), None);
        assert_eq!(normalize_language("  sv  "), Some("sv".to_owned()));
    }

    #[test]
    fn blank_keyterms_are_dropped_before_they_reach_a_providers_cap() {
        let keyterms = vec!["  ".to_owned(), "Sprint".to_owned(), String::new()];
        assert_eq!(normalize_keyterms(&keyterms), vec!["Sprint".to_owned()]);
    }

    #[test]
    fn a_keyterm_count_over_the_cap_is_reported_rather_than_silently_truncated() {
        // What the settings screen renders: AssemblyAI caps at 100 and
        // truncates without complaint on the wire, so this is the data that
        // makes the truncation visible instead.
        let keyterms: Vec<String> = (0..150).map(|i| format!("term{i}")).collect();
        let config = draft_config("assemblyai-realtime", "en", &keyterms).unwrap();
        let review = kw_meeting_core::validate(&config).unwrap();
        assert_eq!(review.keyterms_requested, 150);
        assert_eq!(review.keyterms_used, 100);
        assert_eq!(review.keyterms_dropped, 50);
        assert!(review.has_warnings());
    }

    #[test]
    fn draft_config_refuses_a_provider_nothing_describes() {
        let err = draft_config("not-a-real-provider", "", &[]).unwrap_err();
        assert!(err.contains("not-a-real-provider"), "{err}");
    }
}
