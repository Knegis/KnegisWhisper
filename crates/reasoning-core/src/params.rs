//! Shaping an OpenAI-compatible chat-completions body.
//!
//! This exists because "OpenAI-compatible" is not one API. The token limit is
//! `max_tokens` on some endpoints and `max_completion_tokens` on others; several
//! reasoning models reject `temperature` outright rather than ignoring it. Get
//! either wrong and the provider answers 400, which reads to the user as a
//! broken integration.
//!
//! Upstream centralises this in `chatRequestBody.ts` for a reason worth quoting:
//! so "a family or provider fact changed in the data tables reaches all
//! transports at once instead of one call site at a time". Same principle here —
//! the facts come from the provider registry, and this module is the only place
//! that turns them into parameters.
//!
//! One thing is deliberately *not* ported. Upstream retries with shaped
//! parameters stripped when a provider rejects them by name, logging loudly
//! because "a fallback that fires on every request is a dialect bug to fix, not
//! a rescue". That is a sound safety net, but it is a safety net for a data
//! table that might be wrong; with the registry as a single source of truth and
//! [`ModelShape`] tested against it, the first move is to keep the table right.
//! If a provider turns out to need the retry, it belongs here with a test that
//! names the provider.

use kw_provider_registry::Registry;
use serde_json::{json, Map, Value};

/// Providers whose OpenAI-compatible endpoints speak the legacy shape: always
/// `max_tokens`, always `temperature`.
///
/// Upstream's reasoning, preserved: Groq's registry entries carry no token
/// parameter and its ids do not match the OpenAI heuristics; Tinfoil's catalogue
/// is dynamic, so a model outside the registry must not fall through to
/// `max_completion_tokens`; local and LAN are llama.cpp-style servers.
const LEGACY_SHAPE_PROVIDERS: [&str; 5] = ["local", "lan", "groq", "tinfoil", "corti"];

/// The parameter names and capabilities for one model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelShape {
    /// `max_tokens` or `max_completion_tokens`.
    pub token_param: String,
    pub supports_temperature: bool,
    pub supports_thinking: bool,
}

impl Default for ModelShape {
    /// The legacy shape, which is what an unknown endpoint most likely speaks.
    ///
    /// Defaulting the *other* way would send `max_completion_tokens` to a
    /// llama.cpp server, which ignores it and then generates until it hits its
    /// own limit — a slow, expensive failure rather than an obvious one.
    fn default() -> Self {
        Self {
            token_param: "max_tokens".to_owned(),
            supports_temperature: true,
            supports_thinking: false,
        }
    }
}

/// Look up how to shape a request for `model` from `provider`.
#[must_use]
pub fn shape_for(registry: &Registry, provider: &str, model: &str) -> ModelShape {
    let provider_key = provider.to_lowercase();

    if LEGACY_SHAPE_PROVIDERS.contains(&provider_key.as_str()) {
        return ModelShape::default();
    }

    // The registry knows, where it has an entry.
    if let Ok(entry) = registry.reasoning_provider(&provider_key) {
        if let Some(found) = entry.models.iter().find(|m| m.id == model) {
            return ModelShape {
                token_param: found.token_param.clone(),
                supports_temperature: found.supports_temperature,
                supports_thinking: found.supports_thinking,
            };
        }
    }

    ModelShape::default()
}

/// What a request is for. Decides the default sampling temperature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// A deterministic transform — cleanup, translation, a selection edit.
    /// Temperature zero, because the same input should give the same output.
    Deterministic,
    /// A conversation, where some variation is wanted.
    Conversational,
}

impl Intent {
    #[must_use]
    pub const fn default_temperature(self) -> f64 {
        match self {
            Self::Deterministic => 0.0,
            Self::Conversational => 0.3,
        }
    }
}

/// How a request should be tuned.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestOptions {
    pub intent: Intent,
    /// Overrides the intent's default.
    pub temperature: Option<f64>,
    pub max_tokens: u32,
    /// Ask a thinking-capable model not to think. Ignored by models that cannot.
    pub disable_thinking: bool,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self {
            intent: Intent::Deterministic,
            temperature: None,
            max_tokens: 2_048,
            disable_thinking: false,
        }
    }
}

/// Build the request body.
///
/// `messages` is the caller's; everything tunable is decided here, so a registry
/// fact reaches every transport at once.
#[must_use]
pub fn build_body(
    shape: &ModelShape,
    model: &str,
    messages: Vec<Value>,
    options: &RequestOptions,
) -> Value {
    let mut body = Map::new();
    body.insert("model".to_owned(), json!(model));
    body.insert("messages".to_owned(), json!(messages));

    // Whichever name this endpoint uses -- never both, since sending an unknown
    // parameter is a 400 on strict endpoints.
    body.insert(shape.token_param.clone(), json!(options.max_tokens));

    if shape.supports_temperature {
        let temperature = options
            .temperature
            .unwrap_or_else(|| options.intent.default_temperature());
        body.insert("temperature".to_owned(), json!(temperature));
    }

    // Only meaningful where the model can think. Sending it elsewhere is either
    // ignored or rejected, and neither is worth risking.
    if shape.supports_thinking && options.disable_thinking {
        body.insert("reasoning_effort".to_owned(), json!("none"));
    }

    Value::Object(body)
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

    fn registry() -> &'static Registry {
        kw_provider_registry::registry().expect("the shipped registry must parse")
    }

    fn message() -> Vec<Value> {
        vec![json!({"role": "user", "content": "hello"})]
    }

    /* ---------------------------------------------------------- token params */

    #[test]
    fn the_legacy_providers_always_get_max_tokens() {
        // Groq has no tokenParam in the registry and its ids match no heuristic;
        // Tinfoil's catalogue is dynamic; local and lan are llama.cpp.
        for provider in LEGACY_SHAPE_PROVIDERS {
            let shape = shape_for(registry(), provider, "whatever-model");
            assert_eq!(shape.token_param, "max_tokens", "{provider}");
            assert!(shape.supports_temperature, "{provider}");
        }
    }

    #[test]
    fn a_registry_model_uses_the_parameter_name_the_registry_gives_it() {
        // The whole point: the fact lives in one place.
        let mut checked = 0;
        for provider in &registry().reasoning {
            if LEGACY_SHAPE_PROVIDERS.contains(&provider.id.as_str()) {
                continue;
            }
            for model in &provider.models {
                let shape = shape_for(registry(), &provider.id, &model.id);
                assert_eq!(
                    shape.token_param, model.token_param,
                    "{}/{} disagreed with the registry",
                    provider.id, model.id
                );
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "the registry should have non-legacy models to check"
        );
    }

    #[test]
    fn only_one_token_parameter_is_ever_sent() {
        // Sending both is a 400 on strict endpoints.
        for provider in &registry().reasoning {
            for model in &provider.models {
                let shape = shape_for(registry(), &provider.id, &model.id);
                let body = build_body(&shape, &model.id, message(), &RequestOptions::default());

                let has_max_tokens = body.get("max_tokens").is_some();
                let has_completion = body.get("max_completion_tokens").is_some();
                assert!(
                    has_max_tokens ^ has_completion,
                    "{}/{} sent both or neither: {body}",
                    provider.id,
                    model.id
                );
            }
        }
    }

    #[test]
    fn an_unknown_model_falls_back_to_the_legacy_shape() {
        // Defaulting the other way would send max_completion_tokens to a
        // llama.cpp server, which ignores it and generates until its own limit.
        let shape = shape_for(registry(), "openai", "some-model-released-tomorrow");
        assert_eq!(shape.token_param, "max_tokens");
    }

    #[test]
    fn provider_names_are_matched_case_insensitively() {
        assert_eq!(
            shape_for(registry(), "GROQ", "x"),
            shape_for(registry(), "groq", "x")
        );
    }

    /* ----------------------------------------------------------- temperature */

    #[test]
    fn a_model_that_rejects_temperature_never_receives_it() {
        // Several reasoning models 400 rather than ignoring it.
        let shape = ModelShape {
            token_param: "max_completion_tokens".to_owned(),
            supports_temperature: false,
            supports_thinking: true,
        };
        let body = build_body(&shape, "o-series", message(), &RequestOptions::default());
        assert!(body.get("temperature").is_none(), "{body}");
    }

    #[test]
    fn an_explicit_temperature_is_still_dropped_for_a_model_that_cannot_take_it() {
        // A caller asking for one does not make the provider accept it.
        let shape = ModelShape {
            supports_temperature: false,
            ..ModelShape::default()
        };
        let body = build_body(
            &shape,
            "m",
            message(),
            &RequestOptions {
                temperature: Some(0.9),
                ..Default::default()
            },
        );
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn a_deterministic_transform_gets_temperature_zero() {
        // Cleanup should give the same output for the same input.
        let body = build_body(
            &ModelShape::default(),
            "m",
            message(),
            &RequestOptions {
                intent: Intent::Deterministic,
                ..Default::default()
            },
        );
        assert_eq!(body["temperature"], json!(0.0));
    }

    #[test]
    fn a_conversation_gets_some_variation() {
        let body = build_body(
            &ModelShape::default(),
            "m",
            message(),
            &RequestOptions {
                intent: Intent::Conversational,
                ..Default::default()
            },
        );
        assert_eq!(body["temperature"], json!(0.3));
    }

    #[test]
    fn an_explicit_temperature_overrides_the_intent_default() {
        let body = build_body(
            &ModelShape::default(),
            "m",
            message(),
            &RequestOptions {
                intent: Intent::Deterministic,
                temperature: Some(0.7),
                ..Default::default()
            },
        );
        assert_eq!(body["temperature"], json!(0.7));
    }

    /* -------------------------------------------------------------- thinking */

    #[test]
    fn thinking_is_only_suppressed_on_a_model_that_can_think() {
        let thinker = ModelShape {
            supports_thinking: true,
            ..ModelShape::default()
        };
        let body = build_body(
            &thinker,
            "m",
            message(),
            &RequestOptions {
                disable_thinking: true,
                ..Default::default()
            },
        );
        assert_eq!(body["reasoning_effort"], json!("none"));

        // A model that cannot think must not receive the parameter at all.
        let plain = ModelShape::default();
        let body = build_body(
            &plain,
            "m",
            message(),
            &RequestOptions {
                disable_thinking: true,
                ..Default::default()
            },
        );
        assert!(body.get("reasoning_effort").is_none(), "{body}");
    }

    #[test]
    fn thinking_is_left_alone_unless_explicitly_disabled() {
        let thinker = ModelShape {
            supports_thinking: true,
            ..ModelShape::default()
        };
        let body = build_body(&thinker, "m", message(), &RequestOptions::default());
        assert!(body.get("reasoning_effort").is_none());
    }

    /* ------------------------------------------------------------ the body */

    #[test]
    fn the_body_carries_the_callers_model_and_messages_unchanged() {
        let messages = vec![
            json!({"role": "system", "content": "be brief"}),
            json!({"role": "user", "content": "hello"}),
        ];
        let body = build_body(
            &ModelShape::default(),
            "gpt-4o-mini",
            messages.clone(),
            &RequestOptions::default(),
        );
        assert_eq!(body["model"], json!("gpt-4o-mini"));
        assert_eq!(body["messages"], json!(messages));
    }

    #[test]
    fn the_token_limit_is_the_one_that_was_asked_for() {
        let body = build_body(
            &ModelShape::default(),
            "m",
            message(),
            &RequestOptions {
                max_tokens: 512,
                ..Default::default()
            },
        );
        assert_eq!(body["max_tokens"], json!(512));
    }

    #[test]
    fn no_body_contains_anything_credential_shaped() {
        // Shaping must never be a place a key leaks into.
        for provider in &registry().reasoning {
            for model in &provider.models {
                let shape = shape_for(registry(), &provider.id, &model.id);
                let body = build_body(&shape, &model.id, message(), &RequestOptions::default())
                    .to_string();
                for needle in ["sk-", "gsk_", "Bearer ", "api_key"] {
                    assert!(!body.contains(needle), "{needle} in {body}");
                }
            }
        }
    }
}
