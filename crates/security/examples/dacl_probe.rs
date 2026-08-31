//! Does the DACL this build applies actually take, on this machine?
//!
//! `file_acl`'s judgement is unit-tested on every platform, because
//! `file_acl::classify` is pure. The Win32 half — three calls that each return
//! a `WIN32_ERROR`, a walk over the access list, and the question of whether the
//! descriptor survives a rename — is covered by two `#[cfg(windows)]` tests and
//! by nothing that runs in CI, since the `windows-latest` job is switched off
//! for every trigger but `workflow_dispatch`. This is the probe that closes
//! that gap on real hardware, in the same shape as `credential_probe`.
//!
//! ```text
//! cargo run -p kw-security --example dacl_probe
//! cargo run -p kw-security --example dacl_probe -- "%USERPROFILE%\.knegiswhisper"
//! ```
//!
//! With no argument it creates its own temporary directory and file, applies
//! the DACL to both, renames the file the way `discovery::write` does, and
//! prints what came back. **Given a path it only reads** — so the real
//! `~/.knegiswhisper` can be re-measured without the default being destructive,
//! and without this probe ever being the thing that changed the answer.
//!
//! Nothing here prints a token. The discovery file's *permissions* are the
//! subject; its contents are never read.

fn main() -> std::process::ExitCode {
    let target = std::env::args().nth(1);
    imp::run(target.as_deref())
}

#[cfg(windows)]
mod imp {
    use std::path::Path;
    use std::process::ExitCode;

    use kw_security::file_acl::{self, PathKind, Permissions};

    pub fn run(target: Option<&str>) -> ExitCode {
        match target {
            Some(path) => verify_only(Path::new(path)),
            None => full_cycle(),
        }
    }

    /// Read-only. What the DACL on an existing path is right now.
    fn verify_only(path: &Path) -> ExitCode {
        println!("verifying {} -- nothing is written\n", path.display());
        if !path.exists() {
            println!("there is nothing at that path");
            return ExitCode::FAILURE;
        }
        let kind = if path.is_dir() {
            PathKind::Directory
        } else {
            PathKind::File
        };
        report(path, kind);
        // Read-only: a loose DACL on a path this probe did not create is a
        // finding, not this probe's failure, so the exit code stays success and
        // the output is what says so.
        ExitCode::SUCCESS
    }

    /// Apply, rename, verify. The whole path `discovery::write` takes.
    fn full_cycle() -> ExitCode {
        let Ok(dir) = tempfile::tempdir() else {
            println!("could not create a temporary directory to probe in");
            return ExitCode::FAILURE;
        };
        println!("probing in {}\n", dir.path().display());

        if let Err(err) = file_acl::apply(dir.path(), PathKind::Directory) {
            println!("apply on the directory failed: {err}");
            return ExitCode::FAILURE;
        }
        println!("-- the directory, after apply --");
        report(dir.path(), PathKind::Directory);

        // Written before the DACL is applied to it, exactly as
        // `discovery::write` does not: this is the file created *inside* the
        // already-protected directory, so what it shows is what inheritance
        // gave it.
        let temporary = dir.path().join("local-api.json.new");
        if let Err(err) = std::fs::write(&temporary, "{}") {
            println!("could not write the temporary: {err}");
            return ExitCode::FAILURE;
        }
        println!("\n-- the temporary, inherited from the protected directory only --");
        report(&temporary, PathKind::File);

        if let Err(err) = file_acl::apply(&temporary, PathKind::File) {
            println!("apply on the file failed: {err}");
            return ExitCode::FAILURE;
        }
        println!("\n-- the temporary, after apply --");
        report(&temporary, PathKind::File);

        let final_path = dir.path().join("local-api.json");
        if let Err(err) = std::fs::rename(&temporary, &final_path) {
            println!("could not rename into place: {err}");
            return ExitCode::FAILURE;
        }
        println!("\n-- after the rename, which is the path a caller is told about --");
        let verdict = report(&final_path, PathKind::File);

        let directory = file_acl::verify(dir.path(), PathKind::Directory);
        let combined = file_acl::combine(&directory, &verdict);
        println!("\ncombined with the directory: {combined:?}");

        if combined == Permissions::OwnerAndAdministrators {
            println!("\nthe DACL takes on this machine, and survives the rename.");
            ExitCode::SUCCESS
        } else {
            println!("\nthe DACL did NOT end up as asked for. The entries above say why.");
            ExitCode::FAILURE
        }
    }

    /// Print the read-back and the verdict it produces.
    fn report(path: &Path, kind: PathKind) -> Permissions {
        match file_acl::read_back(path) {
            Ok(facts) => {
                println!(
                    "  owner        {}",
                    if facts.owner.is_empty() {
                        "<none reported>"
                    } else {
                        &facts.owner
                    }
                );
                println!(
                    "  SE_DACL_PROTECTED  {}",
                    match facts.protected {
                        Some(true) => "set",
                        Some(false) => "NOT set -- the descriptor still inherits",
                        None => "GetSecurityDescriptorControl did not answer",
                    }
                );
                match facts.dacl {
                    None => println!("  NULL access list -- grants everything to everybody"),
                    Some(entries) if entries.is_empty() => {
                        println!("  an empty access list -- grants nothing to anybody");
                    }
                    Some(entries) => {
                        for ace in entries {
                            println!(
                                "  {:?} {:<48} mask {:#010x}{}{}",
                                ace.kind,
                                ace.sid,
                                ace.mask,
                                if ace.inherited { " INHERITED" } else { "" },
                                if ace.inheritable { " inheritable" } else { "" },
                            );
                        }
                    }
                }
            }
            Err(err) => println!("  the read-back failed: {err}"),
        }

        let verdict = file_acl::verify(path, kind);
        println!("  verdict      {verdict:?}");
        println!("               {}", verdict.explain());
        verdict
    }
}

#[cfg(not(windows))]
mod imp {
    use std::process::ExitCode;

    pub fn run(target: Option<&str>) -> ExitCode {
        let _ = target;
        // Not a failure. On Unix the mechanism is `chmod` and the read-back is
        // `stat`, both of which the unit tests exercise directly on every run,
        // so there is nothing here a probe would learn that the suite does not
        // already know.
        println!("this probe is about the Windows DACL. On this platform `file_acl` uses chmod,");
        println!("which `cargo test -p kw-security` already applies and reads back.");
        ExitCode::SUCCESS
    }
}
