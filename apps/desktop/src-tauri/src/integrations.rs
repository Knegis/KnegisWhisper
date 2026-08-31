//! `Integrations > CLI` and `Integrations > MCP`.
//!
//! Both screens describe a second front end over the local API rather than a
//! second backend: `tools/knegiswhisper` (the CLI) and
//! `tools/knegiswhisper-mcp` (the stdio MCP server) each talk to the same
//! HTTP surface `local_api.rs` starts, over the same discovery file. Neither
//! binary is bundled with this application, so this module never assumes
//! either exists — [`crate::binary_locator::locate_default`] does a real
//! filesystem check and reports [`crate::binary_locator::BinaryLocation::NotFound`]
//! rather than a reassuring guess when it finds nothing.
//!
//! Both also do nothing at all unless the local API is running, since that is
//! the only thing either of them talks to. This module reads
//! [`LocalApiRuntime`]'s own state for that fact instead of tracking a second
//! copy of it.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use ts_rs::TS;

use crate::binary_locator::{locate_default, BinaryLocation};
use crate::local_api::{DiscoveryPermissions, LocalApiRuntime};

/// The CLI binary's name on this platform.
#[must_use]
pub fn cli_binary_name() -> String {
    format!("knegiswhisper{}", std::env::consts::EXE_SUFFIX)
}

/// The MCP server binary's name on this platform.
#[must_use]
pub fn mcp_binary_name() -> String {
    format!("knegiswhisper-mcp{}", std::env::consts::EXE_SUFFIX)
}

/// What `Integrations > CLI` shows.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    /// The binary's name on this platform, e.g. `knegiswhisper.exe`.
    pub binary_name: String,
    /// Where this actually found it, if anywhere.
    pub binary_location: BinaryLocation,
    /// Where the CLI looks for the port and token. Deterministic, so shown
    /// whether or not the local API is running right now.
    pub discovery_path: String,
    /// Whether the local API is running right now, and so whether the
    /// discovery file exists at all.
    pub local_api_running: bool,
    /// The discovery file's real permission state, evidence and all. `None`
    /// exactly when `local_api_running` is `false`: the file goes with the
    /// server, so there is then nothing to describe.
    pub discovery_permissions: Option<DiscoveryPermissions>,
}

/// Report where the CLI is, and where and how it will find the running
/// application.
#[tauri::command]
pub fn cli_status(api: State<'_, Arc<LocalApiRuntime>>) -> CliStatus {
    // Read now, not remembered. `permissions_now` re-reads the access list on
    // every call, so this screen describes the file as it is rather than as it
    // was when the API started -- see KNEG-110.
    let permissions = api.permissions_now();
    CliStatus {
        binary_name: cli_binary_name(),
        binary_location: locate_default(&cli_binary_name()),
        discovery_path: api.discovery_path().display().to_string(),
        local_api_running: api.is_running(),
        discovery_permissions: permissions.as_ref().map(DiscoveryPermissions::from),
    }
}

/// What `Integrations > MCP` shows.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    /// The binary's name on this platform, e.g. `knegiswhisper-mcp.exe`.
    pub binary_name: String,
    /// Where this actually found it, if anywhere.
    pub binary_location: BinaryLocation,
    /// The value to put in an MCP client's `command` field: the resolved
    /// path when this found the binary, or the bare name otherwise — legible
    /// either way, but nothing here claims a bare name will actually resolve.
    pub command: String,
    /// Whether the local API is running right now. The server is a second
    /// front end over it and does nothing on its own, so a client configured
    /// with `command` will fail every call until this is `true`.
    pub local_api_running: bool,
}

/// Report the exact stdio command line to configure in an MCP client, and
/// whether it would currently work.
#[tauri::command]
pub fn mcp_status(api: State<'_, Arc<LocalApiRuntime>>) -> McpStatus {
    let binary_name = mcp_binary_name();
    let location = locate_default(&binary_name);
    let command = location
        .path()
        .map(str::to_owned)
        .unwrap_or_else(|| binary_name.clone());

    McpStatus {
        binary_name,
        binary_location: location,
        command,
        local_api_running: api.is_running(),
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
    fn binary_names_carry_the_platform_suffix() {
        // Not a hardcoded ".exe": the same source builds on three platforms,
        // and only `EXE_SUFFIX` knows which one this is.
        assert_eq!(
            cli_binary_name(),
            format!("knegiswhisper{}", std::env::consts::EXE_SUFFIX)
        );
        assert_eq!(
            mcp_binary_name(),
            format!("knegiswhisper-mcp{}", std::env::consts::EXE_SUFFIX)
        );
        assert_ne!(cli_binary_name(), mcp_binary_name());
    }
}
