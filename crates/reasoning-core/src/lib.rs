//! Reasoning: chat-completions routing, request shaping, and dispatch.
//!
//! Three modules, and each one exists because of a specific way this goes wrong.
//!
//! [`route`] classifies a provider id. Upstream classified anything it did not
//! recognise as a *local* model, so a cloud provider missing from a hardcoded
//! set produced a request to loopback that failed while blaming the local model.
//! Here the set is derived from the registry and an unknown id is an error.
//!
//! [`params`] decides parameter names. "OpenAI-compatible" is not one API: the
//! token limit is `max_tokens` on some endpoints and `max_completion_tokens` on
//! others, and several reasoning models reject `temperature` rather than
//! ignoring it. Getting either wrong is a 400 that reads as a broken
//! integration.
//!
//! [`chat`] sends the request, and its cleanup path never loses a transcript:
//! tidying dictated text is an improvement, not a requirement.

pub mod chat;
pub mod params;
pub mod route;

pub use chat::{Chat, ChatError, CLEANUP_SYSTEM_PROMPT};
pub use params::{shape_for, Intent, ModelShape, RequestOptions};
pub use route::{cloud_chat_providers, resolve_chat, ChatRoute, ChatRouteError, ChatSettings};
