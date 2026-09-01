//! Which applications are holding the microphone open.
//!
//! # Why this exists
//!
//! Teams runs a call inside `ms-teams.exe` and spawns nothing. Zoom has
//! `CptHost.exe` and Webex has `webexmta.exe` — child processes that exist only
//! during a call — so [`crate::meeting_detect`] can tell "the client is open"
//! from "you are in a call" for those two and cannot for Teams. Measured twice on
//! real hardware, 2026-08-28 and 2026-08-31: with a call in progress the screen
//! read *"open, but not detectably in a call"*, which is the honest answer and
//! also the whole capability for the most common client on this machine.
//!
//! **This is a different question asked of a different subsystem.** Not "what is
//! running" but "what is capturing", which no process table can answer. KNEG-94.
//!
//! # What it does not mean
//!
//! A microphone in use is **not** proof of a meeting. Teams holds the capture
//! device for a test call, a headset app may hold it permanently, and a browser
//! tab holds it for anything at all. This module reports which processes are
//! capturing and takes no view on why; deciding what that implies is
//! [`crate::meeting_detect`]'s job and it needs a second signal — the
//! application being a known meeting client — before it means anything.
//!
//! # It observes. It never records.
//!
//! Nothing here opens a stream, and nothing here can. Enumerating sessions on a
//! capture endpoint reads a state the operating system already publishes to any
//! process; it does not attach to the audio. `docs/SECURITY.md` forbids recording
//! the local user does not know about, and this module must never become a way
//! to notice a microphone and then use it.
//!
//! # The first COM in this workspace
//!
//! `kw-audio-core` reaches WASAPI through `cpal`, which needs no COM of its own,
//! so nothing here called `CoInitializeEx` before this module. That is why
//! `crates/platform/Cargo.toml` gains `Win32_Media_Audio` and `Win32_System_Com`
//! and why the `unsafe` below is the only `unsafe` in this crate outside
//! [`crate::keyhook`].

use serde::Serialize;
use ts_rs::TS;

/// Whether this platform can say which applications are capturing.
///
/// Three states rather than a boolean, for the reason
/// `kw_audio_core::system_audio::SystemAudioSupport` has four: "this platform
/// cannot" and "this platform can and the call failed" are different problems
/// with different fixes, and neither of them is "nothing is capturing".
///
/// That distinction is the whole point here. An empty list of capturing
/// processes and an enumeration that did not run look identical to a caller, and
/// one of them means "nobody is in a call" while the other means nothing at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MicrophoneSupport {
    /// The enumeration ran.
    Available {
        /// The mechanism, named so a reader can recognise it.
        mechanism: &'static str,
        /// How many capture endpoints were examined.
        ///
        /// Zero is not a failure — a machine with no microphone has none — but
        /// it does mean an empty answer says nothing about who is in a call.
        endpoints: usize,
    },
    /// The platform has the mechanism and this attempt failed.
    Failed {
        mechanism: &'static str,
        reason: String,
    },
    /// The platform has no such mechanism.
    Unsupported { reason: String },
}

impl MicrophoneSupport {
    /// Whether an empty list of capturing processes means "nobody is capturing".
    ///
    /// The one question a caller must ask before drawing a conclusion from an
    /// absence, and the reason this type is not a `Vec` and a boolean.
    #[must_use]
    pub const fn absence_is_evidence(&self) -> bool {
        matches!(self, Self::Available { endpoints, .. } if *endpoints > 0)
    }

    /// A short phrase for a settings screen.
    #[must_use]
    pub const fn summary(&self) -> &'static str {
        match self {
            Self::Available { endpoints: 0, .. } => "No microphone to watch",
            Self::Available { .. } => "Available",
            Self::Failed { .. } => "Could not be read",
            Self::Unsupported { .. } => "Not supported on this platform",
        }
    }
}

/// What [`capturing`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capturing {
    /// Lower-cased image names of processes with an **active** capture session.
    ///
    /// Lower-cased at the edge so [`crate::meeting_detect`] compares the way it
    /// already compares process names, rather than each caller remembering to.
    pub processes: Vec<String>,
    /// Whether the enumeration ran, and how far.
    pub support: MicrophoneSupport,
}

/// Which processes currently hold an active capture session.
///
/// The impure half, and deliberately the whole of it — every rule that acts on
/// this lives in [`crate::meeting_detect`] and is tested with no audio hardware
/// in sight, the same split [`crate::keys`]/[`crate::keyhook`] uses.
///
/// `resolve` turns a process id into an image name. Injected rather than called
/// here so the mapping can be tested, and so this does not build a second
/// process table beside the one [`crate::meeting_detect::running`] already
/// builds every five seconds.
#[must_use]
pub fn capturing(resolve: &dyn Fn(u32) -> Option<String>) -> Capturing {
    #[cfg(target_os = "windows")]
    {
        windows_impl::capturing(resolve)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = resolve;
        Capturing {
            processes: Vec::new(),
            support: MicrophoneSupport::Unsupported {
                reason: "only Windows has an audio-session API this project reads; on Linux and \
                         macOS nothing here can say which applications are capturing"
                    .to_owned(),
            },
        }
    }
}

/// Image names by process id, from the same crate `running` uses.
///
/// Built once per call rather than queried per session: `sysinfo` refreshes the
/// whole table either way, and a handful of sessions would otherwise each pay
/// for a walk.
#[must_use]
pub fn process_names() -> std::collections::HashMap<u32, String> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::new());

    system
        .processes()
        .iter()
        .map(|(pid, process)| (pid.as_u32(), process.name().to_string_lossy().into_owned()))
        .collect()
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use windows::core::Interface as _;
    use windows::Win32::Media::Audio::{
        eCapture, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2,
        IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    use super::{Capturing, MicrophoneSupport};

    const MECHANISM: &str = "IAudioSessionManager2 over every active capture endpoint";

    /// Every active capture endpoint, not just the default one.
    ///
    /// A meeting client can be recording from a headset while the default device
    /// is the laptop array, and asking only `GetDefaultAudioEndpoint` would miss
    /// it and report an absence. An absence is the answer that matters here, so
    /// it has to be the expensive one.
    pub(super) fn capturing(resolve: &dyn Fn(u32) -> Option<String>) -> Capturing {
        // SAFETY: COM initialisation for this thread. `CoInitializeEx` returns an
        // HRESULT rather than failing, and RPC_E_CHANGED_MODE means some other
        // component already initialised this thread as an STA -- which is not an
        // error for us, because the calls below work in either apartment.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }

        // Not paired with a `CoUninitialize`. This runs on the meeting
        // detector's polling task, which is a Tokio worker shared with the rest
        // of the application, and tearing down COM on a thread that other
        // components may also have initialised would break them. Leaving a
        // thread initialised is the documented, ordinary thing to do.

        match enumerate(resolve) {
            Ok((processes, endpoints)) => Capturing {
                processes,
                support: MicrophoneSupport::Available {
                    mechanism: MECHANISM,
                    endpoints,
                },
            },
            Err(err) => Capturing {
                processes: Vec::new(),
                support: MicrophoneSupport::Failed {
                    mechanism: MECHANISM,
                    reason: err.to_string(),
                },
            },
        }
    }

    fn enumerate(
        resolve: &dyn Fn(u32) -> Option<String>,
    ) -> windows::core::Result<(Vec<String>, usize)> {
        let mut found: Vec<String> = Vec::new();

        // SAFETY: every pointer below comes from a COM call that returned
        // success, and each interface is dropped by its own `Drop`. Nothing is
        // held across an await -- this function is synchronous on purpose.
        unsafe {
            let devices: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let endpoints = devices.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;
            let count = endpoints.GetCount()?;

            for index in 0..count {
                let device = endpoints.Item(index)?;
                let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
                let sessions = manager.GetSessionEnumerator()?;

                for slot in 0..sessions.GetCount()? {
                    let session = sessions.GetSession(slot)?;

                    // Active, not merely present. An application that opened the
                    // device and stopped keeps an Inactive session, and counting
                    // those would report half the machine as being in a call.
                    if session.GetState()? != AudioSessionStateActive {
                        continue;
                    }

                    // `IAudioSessionControl2` is what carries the process id;
                    // the base interface does not. A session that refuses the
                    // cast is skipped rather than failing the whole
                    // enumeration -- one odd session must not turn a real
                    // answer into "could not be read".
                    let Ok(control) = session.cast::<IAudioSessionControl2>() else {
                        continue;
                    };
                    let Ok(pid) = control.GetProcessId() else {
                        continue;
                    };
                    if let Some(name) = resolve(pid) {
                        let name = name.to_lowercase();
                        if !found.contains(&name) {
                            found.push(name);
                        }
                    }
                }
            }

            Ok((found, usize::try_from(count).unwrap_or(0)))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// An empty answer from a machine with no microphone proves nothing.
    ///
    /// The distinction this type exists for. A caller that treated
    /// `processes.is_empty()` as "nobody is in a call" would be right on a
    /// laptop and wrong on a desktop with the microphone unplugged, and wrong
    /// again whenever the enumeration failed.
    #[test]
    fn an_absence_is_only_evidence_when_something_was_watched() {
        assert!(MicrophoneSupport::Available {
            mechanism: "m",
            endpoints: 1,
        }
        .absence_is_evidence());

        assert!(!MicrophoneSupport::Available {
            mechanism: "m",
            endpoints: 0,
        }
        .absence_is_evidence());

        assert!(!MicrophoneSupport::Failed {
            mechanism: "m",
            reason: "no".to_owned(),
        }
        .absence_is_evidence());

        assert!(!MicrophoneSupport::Unsupported {
            reason: "no".to_owned(),
        }
        .absence_is_evidence());
    }

    /// Every state has its own phrase, and none of them says "nothing is
    /// capturing" — which is the one sentence this type must never produce for
    /// a failure.
    #[test]
    fn no_summary_claims_an_empty_answer_is_a_real_one() {
        let all = [
            MicrophoneSupport::Available {
                mechanism: "m",
                endpoints: 2,
            },
            MicrophoneSupport::Available {
                mechanism: "m",
                endpoints: 0,
            },
            MicrophoneSupport::Failed {
                mechanism: "m",
                reason: "e".to_owned(),
            },
            MicrophoneSupport::Unsupported {
                reason: "e".to_owned(),
            },
        ];

        let phrases: Vec<&str> = all.iter().map(MicrophoneSupport::summary).collect();
        for phrase in &phrases {
            assert!(!phrase.contains("Nothing is capturing"), "{phrase}");
        }
        // All four distinct: a summary that collapsed two states would pass
        // every assertion above and tell the user nothing.
        let unique: std::collections::BTreeSet<&&str> = phrases.iter().collect();
        assert_eq!(unique.len(), phrases.len(), "{phrases:?}");
    }

    /// On a platform without the mechanism, the answer says so rather than
    /// coming back as an ordinary empty list.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn a_platform_without_the_api_says_so() {
        let found = capturing(&|_| None);
        assert!(found.processes.is_empty());
        assert!(matches!(
            found.support,
            MicrophoneSupport::Unsupported { .. }
        ));
        assert!(!found.support.absence_is_evidence());
    }
}
