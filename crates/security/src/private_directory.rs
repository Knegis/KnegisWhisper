//! Private scratch directories. Creation is restrictive from the first instant;
//! existing objects are verified, never chmod'ed or taken over.

#[cfg(windows)]
use std::path::Component;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct PrivateDirectory {
    path: PathBuf,
    // Deny rename/replacement for the lifetime of every path-based writer.
    #[cfg(windows)]
    _pins: Vec<std::fs::File>,
}

impl PrivateDirectory {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Create one private child of an existing trusted directory.
    pub fn create(parent: &Path, name: &str) -> Result<Self, String> {
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err("invalid private directory name".into());
        }
        #[cfg(unix)]
        {
            let parent = parent.canonicalize().map_err(|e| e.to_string())?;
            for ancestor in parent.ancestors() {
                verify_unix_parent(ancestor)?;
            }
            create_unix_leaf(&parent, name)
        }
        #[cfg(windows)]
        {
            if !parent.is_absolute() {
                return Err("private directory parent must be absolute".into());
            }
            let mut pins = Vec::new();
            let mut cumulative = PathBuf::new();
            for component in parent.components() {
                cumulative.push(component);
                match component {
                    Component::Prefix(_) => continue,
                    Component::ParentDir | Component::CurDir => {
                        return Err("invalid private directory parent".into())
                    }
                    _ => {}
                }
                pins.push(pin_windows_directory(&cumulative)?);
            }
            let path = parent.join(name);
            crate::file_acl::create_private_directory(&path)?;
            let leaf = pin_windows_directory(&path)?;
            crate::file_acl::verify_private_directory_handle(&leaf)?;
            pins.push(leaf);
            Ok(Self { path, _pins: pins })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (parent, name);
            Err("private scratch directories are unsupported on this platform".into())
        }
    }
}

/// Stable ASCII discriminator; no account names or profile paths in shared names.
pub fn scratch_name(profile: &Path) -> Result<String, String> {
    use sha2::{Digest as _, Sha256};
    #[cfg(unix)]
    // SAFETY: geteuid has no arguments or memory requirements.
    let user = unsafe { libc::geteuid() }.to_string();
    #[cfg(windows)]
    let user = crate::file_acl::current_user_sid()?;
    #[cfg(not(any(unix, windows)))]
    let user = "unsupported".to_owned();
    let mut hash = Sha256::new();
    hash.update(user.as_bytes());
    hash.update([0]);
    hash.update(profile.as_os_str().as_encoded_bytes());
    Ok(format!("knegiswhisper-audio-v1-{:x}", hash.finalize()))
}

#[cfg(unix)]
fn verify_unix_parent(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt as _;
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    // SAFETY: geteuid has no arguments or memory requirements.
    let uid = unsafe { libc::geteuid() };
    if !meta.is_dir()
        || (meta.uid() != 0 && meta.uid() != uid)
        || (meta.mode() & 0o022 != 0 && meta.mode() & 0o1000 == 0)
    {
        return Err("scratch parent is not a trusted directory".into());
    }
    Ok(())
}

#[cfg(unix)]
fn create_unix_leaf(parent: &Path, name: &str) -> Result<PrivateDirectory, String> {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};
    let path = parent.join(name);
    match std::fs::DirBuilder::new().mode(0o700).create(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.to_string()),
    }
    let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    // SAFETY: geteuid has no arguments or memory requirements.
    let uid = unsafe { libc::geteuid() };
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o777 != 0o700 {
        return Err("scratch directory is not private and owned by this user".into());
    }
    Ok(PrivateDirectory { path })
}

#[cfg(windows)]
fn pin_windows_directory(path: &Path) -> Result<std::fs::File, String> {
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_GENERIC_READ, FILE_SHARE_READ,
    };
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_GENERIC_READ.0)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err("scratch ancestor is not a directory or is a reparse point".into());
    }
    Ok(file)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // Test leaf creation on a real Unix filesystem even when the repository
    // lives on DrvFS. Production create() additionally verifies all ancestors.
    fn create_fixture(parent: &Path, name: &str) -> Result<PrivateDirectory, String> {
        #[cfg(unix)]
        {
            verify_unix_parent(parent)?;
            create_unix_leaf(parent, name)
        }
        #[cfg(not(unix))]
        {
            PrivateDirectory::create(parent, name)
        }
    }

    #[test]
    fn private_creation_reuse_and_external_writes() {
        let parent = tempfile::tempdir().unwrap();
        let dir = create_fixture(parent.path(), "audio").unwrap();
        std::fs::write(dir.path().join("converted.wav"), b"synthetic audio").unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("converted.wav")).unwrap(),
            b"synthetic audio"
        );
        #[cfg(unix)]
        assert!(crate::file_acl::verify(dir.path(), crate::PathKind::Directory).is_confirmed());
        #[cfg(windows)]
        assert!(crate::file_acl::verify(dir.path(), crate::PathKind::Directory).is_confirmed());
        assert!(create_fixture(parent.path(), "audio").is_ok());
        assert_ne!(
            scratch_name(Path::new("profile-a")).unwrap(),
            scratch_name(Path::new("profile-b")).unwrap()
        );
        assert!(scratch_name(Path::new("Håkan")).unwrap().is_ascii());
        drop(dir);
        std::fs::remove_dir_all(parent.path().join("audio")).unwrap();
    }

    #[test]
    fn refuses_preexisting_files_and_directory_links() {
        let parent = tempfile::tempdir().unwrap();
        std::fs::write(parent.path().join("file"), b"keep").unwrap();
        assert!(create_fixture(parent.path(), "file").is_err());
        let target = parent.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = parent.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .unwrap()
            .status
            .success());
        assert!(create_fixture(parent.path(), "link").is_err());
        #[cfg(windows)]
        assert!(PrivateDirectory::create(&link, "child").is_err());
        #[cfg(windows)]
        std::fs::remove_dir(&link).unwrap();
    }

    #[test]
    #[ignore = "requires KW_SCRATCH_FFMPEG pointing to an installed ffmpeg; creates synthetic silence only"]
    fn native_writer_can_create_audio_while_the_directory_is_pinned() {
        let parent = tempfile::tempdir().unwrap();
        let name = scratch_name(Path::new("non-ascii-profile-Håkan")).unwrap();
        let dir = create_fixture(parent.path(), &name).unwrap();
        let audio = dir.path().join("converted.wav");
        let result = std::process::Command::new(std::env::var_os("KW_SCRATCH_FFMPEG").unwrap())
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=channel_layout=mono:sample_rate=16000",
                "-t",
                "0.1",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&audio)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(std::fs::read(&audio).unwrap().starts_with(b"RIFF"));
    }

    #[cfg(windows)]
    #[test]
    fn long_stable_scratch_names_can_be_created() {
        let parent = tempfile::tempdir().unwrap();
        let name = scratch_name(Path::new("profile-with-non-ascii-Håkan")).unwrap();
        let dir = PrivateDirectory::create(parent.path(), &name).unwrap();
        std::fs::write(dir.path().join("converted.wav"), b"fixture").unwrap();
        assert!(PrivateDirectory::create(parent.path(), &name).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn pinned_directory_cannot_be_replaced_but_children_can_be_removed() {
        let parent = tempfile::tempdir().unwrap();
        let dir = PrivateDirectory::create(parent.path(), "audio").unwrap();
        assert!(std::fs::rename(dir.path(), parent.path().join("replacement")).is_err());
        std::fs::create_dir(dir.path().join("job")).unwrap();
        std::fs::write(dir.path().join("job/converted.wav"), b"fixture").unwrap();
        std::fs::remove_dir_all(dir.path().join("job")).unwrap();
        drop(dir);
        std::fs::rename(
            parent.path().join("audio"),
            parent.path().join("replacement"),
        )
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_wider_directories_and_untrusted_ancestors_without_repairing() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let parent = tempfile::tempdir().unwrap();
        let wide = parent.path().join("wide");
        std::fs::create_dir(&wide).unwrap();
        std::fs::set_permissions(&wide, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(create_fixture(parent.path(), "wide").is_err());
        assert_eq!(std::fs::metadata(&wide).unwrap().mode() & 0o777, 0o755);
        std::fs::set_permissions(&wide, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(verify_unix_parent(&wide).is_err());
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires root and a real Unix filesystem; uses synthetic data and numeric UID 65534"]
    fn second_uid_cannot_read_or_replace_audio() {
        use std::os::unix::{fs::PermissionsExt as _, process::CommandExt as _};
        // SAFETY: geteuid has no arguments or memory requirements.
        assert_eq!(unsafe { libc::geteuid() }, 0);
        let parent = tempfile::tempdir().unwrap();
        std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o1777)).unwrap();
        let dir = create_fixture(parent.path(), "audio").unwrap();
        let wav = dir.path().join("converted.wav");
        std::fs::write(&wav, b"synthetic private recording").unwrap();
        std::fs::set_permissions(&wav, std::fs::Permissions::from_mode(0o644)).unwrap();
        for (command, argument) in [
            ("cat", wav.as_path()),
            ("ls", dir.path()),
            ("rm", wav.as_path()),
        ] {
            assert!(!std::process::Command::new(command)
                .arg(argument)
                .uid(65534)
                .gid(65534)
                .output()
                .unwrap()
                .status
                .success());
        }
        let foreign = parent.path().join("foreign");
        assert!(std::process::Command::new("mkdir")
            .args(["-m", "700"])
            .arg(&foreign)
            .uid(65534)
            .gid(65534)
            .status()
            .unwrap()
            .success());
        assert!(create_fixture(parent.path(), "foreign").is_err());
        let old = parent.path().join("old-public.wav");
        std::fs::write(&old, b"synthetic former behavior").unwrap();
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(std::process::Command::new("cat")
            .arg(&old)
            .uid(65534)
            .gid(65534)
            .output()
            .unwrap()
            .status
            .success());
    }
}
