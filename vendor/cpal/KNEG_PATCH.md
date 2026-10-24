# CPAL 0.15.3 WASAPI lifetime patch

Source: crates.io `cpal` 0.15.3, archive SHA-256
`873dab07c8f743075e57f524c583985fbaf745602acbe916a01539364369a779`.
Upstream's Apache-2.0 license is preserved in `LICENSE`.

The functional change is confined to `src/host/wasapi/device.rs`:
create an owned `IMMDeviceEnumerator` per call instead of caching the first
thread's apartment-bound interface for the process lifetime. Initialize COM
on each calling thread and release the enumerator before that thread exits.
Enumerator creation errors become normal device-enumeration errors or an
unavailable default device, rather than an unwrap panic.
The existing MutexGuard return type also spells out its elided lifetime to
avoid the Rust 1.98 lifetime-syntax warning; this has no runtime effect.
The unlimited buffer-size range writes `u32::MAX` for upstream's deprecated
`u32::max_value()`, which clippy on Rust 1.99 warns about (KNEG-486); the same
value, so this has no runtime effect either.
Two trailing whitespace runs in `src/lib.rs` documentation were removed for
the repository's diff check. All other upstream source files are unchanged.

KNEG-178 exposed STATUS_ACCESS_VIOLATION in hosted Windows device-probe tests.
The same lifetime defect is described in
[upstream issue 1302](https://github.com/RustAudio/cpal/issues/1302).
The additional DefaultDeviceMonitor path discussed there does not exist in
0.15.3. We keep the existing audio API/dependency versions rather than assuming
that a newer version has fixed this still-open defect.

The root Cargo patch applies this dependency to production and tests alike.
`crates/audio-core/tests/wasapi_thread_lifetime.rs` exercises sequential COM
owner teardown in its own process. It passes on the development machine even
without this patch; hosted verification remains necessary for the failing
environment. It is not a test to skip or mark ignored on Windows.

The upstream examples, manifest, README and build script are retained for
package integrity; upstream CI configuration and registry-local marker files
are not included. Remove this patch only after a replacement CPAL revision
fixes the lifetime defect and passes the same Windows checks.
