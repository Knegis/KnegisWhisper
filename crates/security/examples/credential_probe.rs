//! Does the real OS credential store work on this machine?
//!
//! Every test of [`CredentialStore`] runs against `MemoryCredentialStore`, on
//! purpose — a test suite must not write to the developer's keychain. The
//! consequence is that `OsCredentialStore`, the only implementation the
//! application actually uses, is covered by no test at all. This is the probe
//! that closes that gap, in the same shape as `system_audio_probe` and
//! `keyhook_probe`: run it on real hardware and read what it says.
//!
//! ```text
//! cargo run -p kw-security --example credential_probe
//! ```
//!
//! It uses a slot named `probe-delete-me`, which is not a slot the provider
//! registry declares, so it cannot collide with a stored provider key. The value
//! is a fixed non-secret and is deleted again on the way out. **Nothing here
//! prints a secret** — the round trip is checked by comparison, and what is
//! reported is whether the comparison held.

use kw_security::{CredentialError, CredentialStore as _, KeyRef, OsCredentialStore, Secret};

fn main() -> std::process::ExitCode {
    let store = OsCredentialStore::new();

    let Some(key) = KeyRef::new("probe-delete-me") else {
        println!("the probe's own slot name was refused, which is a bug in the probe");
        return std::process::ExitCode::FAILURE;
    };

    // Left over from an interrupted run: cleared first so "already present" does
    // not read as a successful write.
    if let Err(err) = store.delete(&key) {
        println!("could not clear the probe slot first: {err}");
        return report(&store, &key);
    }
    if store.contains(&key) {
        println!("the probe slot still reports present after a delete");
        return std::process::ExitCode::FAILURE;
    }
    println!("absent before the write, as expected");

    // Not a credential, and not secret. A real key is never needed to find out
    // whether the store works.
    let written = Secret::new("probe-value-not-a-credential".to_owned());
    if let Err(err) = store.set(&key, &written) {
        println!("write failed: {err}");
        return report(&store, &key);
    }
    println!("wrote a value to {}/{key}", kw_security::SERVICE);

    if !store.contains(&key) {
        println!("`contains` says no immediately after a successful write");
        return std::process::ExitCode::FAILURE;
    }
    println!("`contains` agrees it is there");

    match store.get(&key) {
        // Compared, not printed. A probe that echoes what it stored is a probe
        // that would echo a real credential the day somebody points it at one.
        Ok(read) if read.expose() == written.expose() => {
            println!("read back the same value");
        }
        Ok(_) => {
            println!("read back a DIFFERENT value than was written");
            let _ = store.delete(&key);
            return std::process::ExitCode::FAILURE;
        }
        Err(err) => {
            println!("read failed after a successful write: {err}");
            let _ = store.delete(&key);
            return std::process::ExitCode::FAILURE;
        }
    }

    if let Err(err) = store.delete(&key) {
        println!("delete failed, so the probe slot is still on this machine: {err}");
        return std::process::ExitCode::FAILURE;
    }
    if store.contains(&key) {
        println!("still present after delete");
        return std::process::ExitCode::FAILURE;
    }
    println!("deleted, and absent again");

    // The distinction the application relies on: a missing credential is
    // `NotFound`, not `NoBackend`. Confusing the two turns "you have not entered
    // a key" into "this machine has no keychain", which sends the user somewhere
    // entirely unhelpful.
    match store.get(&key) {
        Err(CredentialError::NotFound(_)) => println!("a missing credential reports NotFound"),
        Err(other) => {
            println!("a missing credential reported {other} rather than NotFound");
            return std::process::ExitCode::FAILURE;
        }
        Ok(_) => {
            println!("reading a deleted credential succeeded");
            return std::process::ExitCode::FAILURE;
        }
    }

    println!("\nthe OS credential store works on this machine.");
    std::process::ExitCode::SUCCESS
}

/// Say which failure this is, because the two need different answers.
fn report(store: &OsCredentialStore, key: &KeyRef) -> std::process::ExitCode {
    match store.get(key) {
        Err(CredentialError::NoBackend { detail }) => {
            println!("there is no usable credential store here: {detail}");
            println!("on Linux that means no Secret Service provider is running.");
        }
        _ => println!("the store is reachable, so the failure above is not a missing backend."),
    }
    std::process::ExitCode::FAILURE
}
