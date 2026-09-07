//! Narrowing who may read and write one file on disk, and reading the result
//! back before saying anything about it.
//!
//! Two callers, three paths, one mechanism. `kw_local_api::discovery` writes
//! the local API's bearer token to `~/.knegiswhisper/local-api.json`, and
//! `kw_sidecar::pid` writes a record per sidecar it starts. The token is a
//! secret and the PID record is not — a PID is public — but the two need the
//! same thing from the filesystem for different reasons. The token must not be
//! **readable** by another local user, because reading it is using the API. The
//! PID record must not be **writable** by one, because a record another user
//! could rewrite is a request to signal a process of their choosing.
//!
//! # What is asked for
//!
//! On Unix, `0600` for a file and `0700` for a directory.
//!
//! On Windows, an explicit DACL granting three principals and nobody else:
//!
//! - **the process's token user** — this application, so its next start can
//!   still read and rewrite what it wrote;
//! - **`NT AUTHORITY\SYSTEM`** (`S-1-5-18`);
//! - **`BUILTIN\Administrators`** (`S-1-5-32-544`).
//!
//! Not owner-only. An administrator can take ownership of any object and
//! re-grant themselves anything, so a DACL that excludes them is theatre that
//! costs backup and repair tooling real breakage. The trustees worth removing
//! are the *unexpected* ones — the entries a profile directory hands down that
//! nobody chose — and the inheritance that keeps handing them down.
//!
//! The grant is derived from the process token rather than from the object's
//! owner on purpose. If machine policy makes `BUILTIN\Administrators` the owner
//! of objects an elevated process creates, an owner-derived DACL would list
//! Administrators and *not* the user, and a later non-elevated run could not
//! read its own file. The owner is still read back and judged: an owner outside
//! the three always holds `READ_CONTROL` and `WRITE_DAC` and can re-grant
//! itself, so it counts as an extra trustee whether or not it has an entry.
//!
//! Ownership is never taken and the SACL is never touched.
//!
//! # Why it is read back
//!
//! Setting a mode and reporting success is the mistake this crate exists to not
//! make. `chmod` is ignored on some filesystems; a DACL applied to a path on a
//! network redirector does not behave like one in a home directory. So [`apply`]
//! and [`verify`] are separate, [`verify`] asks the operating system what the
//! object actually says, and [`classify`] — pure, and therefore testable on
//! every platform including the Linux box that runs CI — turns that answer into
//! a [`Permissions`] verdict. Nothing is reported that was not read back.

use std::path::Path;

/// Whether the path being protected is a file or a directory holding one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    File,
    Directory,
}

impl PathKind {
    /// The Unix mode this kind asks for.
    #[cfg(unix)]
    const fn mode(self) -> u32 {
        match self {
            Self::File => 0o600,
            Self::Directory => 0o700,
        }
    }
}

/// What the filesystem actually granted.
///
/// Never a boolean: "restricted" and "we asked for it to be restricted" are
/// different claims, and only the first is worth telling a user. Every arm
/// other than [`Self::InheritedFromProfile`] and [`Self::Unknown`] is the
/// result of a read-back.
///
/// The sentences [`Self::explain`] produces are deliberately free of any
/// mention of the local API. Two crates report through this vocabulary and one
/// of them is describing a PID record; the caller's framing belongs at the
/// caller — in `local_api.rs`'s warning and in `labels.ts` — not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Permissions {
    /// Unix. Verified by reading the mode back: owner only.
    OwnerOnly,
    /// Windows. Verified by reading the access list back: the process's user,
    /// `NT AUTHORITY\SYSTEM`, `BUILTIN\Administrators`, and nothing else, with
    /// nothing inherited.
    OwnerAndAdministrators,
    /// Nothing was applied and nothing was read back, because this platform has
    /// neither mechanism. The honest answer for a target that is neither Unix
    /// nor Windows.
    InheritedFromProfile,
    /// Unix. The mode was set and read back as something other than the one
    /// that was set.
    Wider { mode: u32 },
    /// Windows. The access list read back grants principals outside the three
    /// that were asked for. The count rather than the names: a trustee name is
    /// a machine's account and group names, and this value reaches a settings
    /// screen that ends up in screenshots and bug reports. The names go to the
    /// log, where the person who needs them can get them and a screenshot
    /// cannot.
    ExtraTrustees { count: usize },
    /// Windows. The three principals are right, but the list is still being
    /// inherited from the directory above — so what it grants is whatever that
    /// directory grants, and it can change without anything here being asked.
    StillInheriting,
    /// Windows. The access list grants a group that means "everyone on this
    /// machine", or there is no access list at all — which is not "no access",
    /// it is *no check*, and grants everything to everybody.
    GrantsEveryone,
    /// Windows. Nothing unexpected is in the list, but what was asked for is
    /// not all there: a principal is missing, denied, or granted less than it
    /// was given.
    Incomplete,
    /// The object's own permissions are right and the directory holding it is
    /// not restricted, so it can be deleted and replaced by anything that can
    /// write there. See [`combine`].
    DirectoryNotRestricted,
    /// The permissions could not be applied, or could not be read back.
    Unknown { reason: String },
}

impl Permissions {
    /// A sentence for a settings screen or a log line.
    #[must_use]
    pub fn explain(&self) -> String {
        match self {
            Self::OwnerOnly => {
                "readable only by your user account, verified by reading the mode back".to_owned()
            }
            Self::OwnerAndAdministrators => "readable by your user account, the system and this \
                 machine's administrators, and by nobody else — verified by reading the access \
                 list back"
                .to_owned(),
            Self::InheritedFromProfile => "of whatever permissions the directory holding it \
                 happens to give: nothing here narrowed them, and nothing here read them back"
                .to_owned(),
            Self::Wider { mode } => format!(
                "readable more widely than intended: the mode reads back as {mode:o}, not the \
                 owner-only one that was set"
            ),
            Self::ExtraTrustees { count } => format!(
                "readable by {count} account(s) or group(s) beyond your own, the system and this \
                 machine's administrators"
            ),
            Self::StillInheriting => "still inheriting its access list from the directory above \
                 it, so what it grants is whatever that directory grants and can change without \
                 notice"
                .to_owned(),
            Self::GrantsEveryone => {
                "readable by every account on this machine — there is no restriction on it at all"
                    .to_owned()
            }
            Self::Incomplete => "not the access list that was asked for: what read back does not \
                 grant your account, the system and the administrators what was set"
                .to_owned(),
            Self::DirectoryNotRestricted => "in a directory that is not restricted, so anything \
                 that can write there can delete it and put its own file in its place, whatever \
                 this file's own permissions say"
                .to_owned(),
            Self::Unknown { reason } => format!(
                "of unknown permissions: {reason}. Assume anything running as you can read it."
            ),
        }
    }

    /// Whether this is a restriction that was **checked**, not merely one that
    /// was asked for.
    ///
    /// The line is checked versus unchecked, not strict versus lax.
    /// [`Self::OwnerAndAdministrators`] is on the confirmed side even though
    /// administrators can read the object, because that grant is deliberate and
    /// was read back. [`Self::InheritedFromProfile`] is on the other side even
    /// though it may well be fine, because nothing looked.
    #[must_use]
    pub const fn is_confirmed(&self) -> bool {
        matches!(self, Self::OwnerOnly | Self::OwnerAndAdministrators)
    }
}

/// The verdict for an object, given what the directory holding it read back as.
///
/// A file whose own access list is exactly right, inside a directory another
/// user may delete children of, is **not** a restricted file: it can be deleted
/// and replaced, and whatever reads it next reads the replacement. That is not
/// a smaller problem than a readable file — for the discovery file it is a
/// larger one, because it turns reading a token into supplying one.
///
/// Pure, and unit-tested, because it is the one place the two verdicts meet.
#[must_use]
pub fn combine(directory: &Permissions, file: &Permissions) -> Permissions {
    if !file.is_confirmed() {
        // The object's own problem is the more specific of the two; a caller
        // shown "the directory is loose" would go and fix the wrong thing.
        file.clone()
    } else if directory.is_confirmed() {
        file.clone()
    } else {
        Permissions::DirectoryNotRestricted
    }
}

/// What one entry in an access list does, as far as the verdict is concerned.
///
/// An enum rather than the raw `AceType` byte so that the two Win32 constants
/// live in exactly one place — the module that has them — and the classifier,
/// which is compiled on every platform, has no magic numbers in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AceKind {
    /// `ACCESS_ALLOWED_ACE_TYPE`.
    Allow,
    /// `ACCESS_DENIED_ACE_TYPE`.
    Deny,
    /// Any other type. Its layout is not the one the SID is read out of here,
    /// so it is recorded as an entry whose trustee is unknown rather than
    /// parsed and guessed at — and counted against the verdict, because "this
    /// was not understood" must not read as "this was fine".
    Unrecognised { ace_type: u8 },
}

/// One entry from an access list as read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AceFacts {
    /// The trustee's security identifier in canonical string form, e.g.
    /// `S-1-5-18`. A SID and not a resolved account name: the comparison is
    /// exact, it needs no lookup that can fail or hang on a domain-joined
    /// machine, and `NT AUTHORITY\SYSTEM` is spelled differently per locale.
    /// Empty for an [`AceKind::Unrecognised`] entry.
    pub sid: String,
    pub kind: AceKind,
    /// `INHERITED_ACE`: this entry came from the directory above rather than
    /// being set on this object.
    pub inherited: bool,
    /// `OBJECT_INHERIT_ACE` or `CONTAINER_INHERIT_ACE`: this entry is handed
    /// down to things created inside. Wanted on a directory, not on a file.
    pub inheritable: bool,
    pub mask: u32,
}

/// A security descriptor as read back, reduced to the facts the verdict turns
/// on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaclFacts {
    /// `SE_DACL_PROTECTED` as reported by `GetSecurityDescriptorControl`, or
    /// `None` when that call did not answer.
    ///
    /// `None` is not `false`. If the control bits cannot be read, inheritance
    /// is judged from the entries' own `INHERITED_ACE` flags, which is the fact
    /// that actually matters — rather than reporting a problem nothing observed.
    pub protected: Option<bool>,
    /// The object's owner as a SID string. Judged as a trustee in its own
    /// right: an owner holds `WRITE_DAC` whatever the list says.
    pub owner: String,
    /// The entries, or `None` for a NULL access list.
    ///
    /// The `Option` is the point. A NULL list grants **everyone** everything; an
    /// empty list grants **nobody** anything. Code that conflates them reports
    /// the worst case as the best one.
    pub dacl: Option<Vec<AceFacts>>,
}

/// The access list that was asked for, as plain facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requested {
    /// SID strings that may appear, and nothing else may.
    pub allowed: Vec<String>,
    /// The access mask each of `allowed` was granted.
    pub mask: u32,
    /// Whether the entries should be handed down to things created inside.
    /// True for a directory, false for a file.
    pub inheritable: bool,
}

/// Groups that mean "everyone on this machine" for this purpose.
///
/// An allow entry naming one of these is not one unexpected trustee, it is the
/// absence of any restriction, and saying that is more use than a count of one.
const WORLD_SIDS: [&str; 5] = [
    "S-1-1-0",      // Everyone
    "S-1-5-7",      // Anonymous Logon
    "S-1-5-11",     // Authenticated Users
    "S-1-5-32-545", // BUILTIN\Users
    "S-1-5-32-546", // BUILTIN\Guests
];

/// Turn a read-back into a verdict.
///
/// Pure on purpose. The Win32 call that produces [`DaclFacts`] cannot run in
/// CI, and the judgement is the part worth testing — the same split
/// `check_importable_file` and `import_pick_dialog` use in the desktop crate.
/// The measured exposure from `docs/BENCH_CHECK.md` is a table of strings that
/// can be fed straight in, so the regression test for it runs on Linux.
///
/// Worst first: a verdict names the most serious thing that is true, so a file
/// that both grants Everyone and is still inheriting reports the grant.
#[must_use]
pub fn classify(facts: &DaclFacts, requested: &Requested) -> Permissions {
    let Some(entries) = facts.dacl.as_ref() else {
        return Permissions::GrantsEveryone;
    };

    let allowed = |sid: &str| requested.allowed.iter().any(|candidate| candidate == sid);

    if entries
        .iter()
        .any(|ace| ace.kind == AceKind::Allow && WORLD_SIDS.contains(&ace.sid.as_str()))
    {
        return Permissions::GrantsEveryone;
    }

    let extra = usize::from(!allowed(&facts.owner))
        + entries
            .iter()
            .filter(|ace| match ace.kind {
                AceKind::Allow => !allowed(&ace.sid),
                // A deny entry naming somebody outside the set grants nothing,
                // so it is not a trustee that can read this.
                AceKind::Deny => false,
                AceKind::Unrecognised { .. } => true,
            })
            .count();
    if extra > 0 {
        return Permissions::ExtraTrustees { count: extra };
    }

    if facts.protected == Some(false) || entries.iter().any(|ace| ace.inherited) {
        return Permissions::StillInheriting;
    }

    let granted = |sid: &str| {
        entries.iter().any(|ace| {
            ace.kind == AceKind::Allow
                && ace.sid == sid
                && ace.mask & requested.mask == requested.mask
                && ace.inheritable == requested.inheritable
        })
    };
    let denied = |sid: &str| {
        entries
            .iter()
            .any(|ace| ace.kind == AceKind::Deny && ace.sid == sid)
    };
    if !requested
        .allowed
        .iter()
        .all(|sid| granted(sid) && !denied(sid))
    {
        return Permissions::Incomplete;
    }

    Permissions::OwnerAndAdministrators
}

/// Apply the strictest sharing this platform offers to `path`.
///
/// Separate from [`verify`] because the two happen at different moments: a
/// discovery file is restricted while it is still a temporary, and verified
/// after it has been renamed into place. A single call that did both would be
/// describing a path that no longer exists.
///
/// # Errors
///
/// The operating system's own message, when the call failed.
pub fn apply(path: &Path, kind: PathKind) -> Result<(), String> {
    #[cfg(unix)]
    {
        unix_impl::apply(path, kind)
    }

    #[cfg(target_os = "windows")]
    {
        windows_impl::apply(path, kind)
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        // Neither `chmod` nor a DACL here. Nothing is applied, and [`verify`]
        // says so rather than this returning a success it did not achieve.
        let _ = (path, kind);
        Ok(())
    }
}

/// Ask the operating system what `path` actually grants, and judge the answer.
#[must_use]
pub fn verify(path: &Path, kind: PathKind) -> Permissions {
    #[cfg(unix)]
    {
        unix_impl::verify(path, kind)
    }

    #[cfg(target_os = "windows")]
    {
        windows_impl::verify(path, kind)
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let _ = (path, kind);
        Permissions::InheritedFromProfile
    }
}

#[cfg(unix)]
mod unix_impl {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::Path;

    use super::{PathKind, Permissions};

    pub(super) fn apply(path: &Path, kind: PathKind) -> Result<(), String> {
        fs::set_permissions(path, fs::Permissions::from_mode(kind.mode()))
            .map_err(|err| err.to_string())
    }

    pub(super) fn verify(path: &Path, kind: PathKind) -> Permissions {
        match fs::metadata(path) {
            Ok(metadata) => {
                let mode = metadata.permissions().mode() & 0o777;
                if mode == kind.mode() {
                    Permissions::OwnerOnly
                } else {
                    Permissions::Wider { mode }
                }
            }
            Err(err) => Permissions::Unknown {
                reason: err.to_string(),
            },
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::os::windows::ffi::OsStrExt as _;
    use std::path::Path;

    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, LocalFree, ERROR_SUCCESS, HANDLE, HLOCAL, WIN32_ERROR,
    };
    use windows::Win32::Security::Authorization::{
        ConvertSidToStringSidW, GetNamedSecurityInfoW, SetEntriesInAclW, SetNamedSecurityInfoW,
        EXPLICIT_ACCESS_W, NO_MULTIPLE_TRUSTEE, SET_ACCESS, SE_FILE_OBJECT, TRUSTEE_IS_SID,
        TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
    };
    use windows::Win32::Security::{
        CopySid, CreateWellKnownSid, GetAce, GetSecurityDescriptorControl, GetTokenInformation,
        TokenUser, WinBuiltinAdministratorsSid, WinLocalSystemSid, ACCESS_ALLOWED_ACE, ACE_HEADER,
        ACL, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, INHERITED_ACE, NO_INHERITANCE,
        OBJECT_INHERIT_ACE, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, SECURITY_MAX_SID_SIZE, SE_DACL_PROTECTED,
        SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY, TOKEN_USER, WELL_KNOWN_SID_TYPE,
    };
    use windows::Win32::Storage::FileSystem::FILE_ALL_ACCESS;
    use windows::Win32::System::SystemServices::{ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    use super::{AceFacts, AceKind, DaclFacts, PathKind, Permissions, Requested};

    /// `SECURITY_MAX_SID_SIZE` bytes, counted in `u32` words.
    const SID_WORDS: usize = (SECURITY_MAX_SID_SIZE as usize).div_ceil(4);

    /// A SID copied into a buffer this code owns.
    ///
    /// `u32` words rather than bytes because a `SID` carries 32-bit
    /// sub-authorities and must be 4-byte aligned, which a `[u8; N]` is not.
    #[derive(Clone, Copy)]
    struct SidBuffer {
        words: [u32; SID_WORDS],
    }

    impl SidBuffer {
        const fn zeroed() -> Self {
            Self {
                words: [0; SID_WORDS],
            }
        }

        /// A pointer for the Win32 calls that only read through it. The cast
        /// away from `const` is sound because nothing writes through the
        /// result — the one call that does takes [`Self::psid_mut`].
        fn psid(&self) -> PSID {
            PSID(self.words.as_ptr().cast_mut().cast())
        }

        fn psid_mut(&mut self) -> PSID {
            PSID(self.words.as_mut_ptr().cast())
        }
    }

    /// The three principals the access list is built for.
    struct Grants {
        sids: [SidBuffer; 3],
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn check(status: WIN32_ERROR, api: &str) -> Result<(), String> {
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!(
                "{api} failed with {} ({})",
                status.0,
                status.to_hresult().message()
            ))
        }
    }

    fn sid_string(sid: PSID) -> Result<String, String> {
        let mut raw = PWSTR::null();
        // SAFETY: `sid` points at a valid SID for the duration of the call, and
        // `raw` receives a `LocalAlloc`ed string freed below.
        unsafe { ConvertSidToStringSidW(sid, &mut raw) }
            .map_err(|err| format!("ConvertSidToStringSidW failed: {err}"))?;
        if raw.is_null() {
            return Err(
                "ConvertSidToStringSidW reported success and returned no string".to_owned(),
            );
        }
        // SAFETY: `raw` is a null-terminated UTF-16 string the call just wrote.
        let text = unsafe { raw.to_string() };
        // SAFETY: `raw` came from `LocalAlloc` inside `ConvertSidToStringSidW`,
        // and is not used again.
        unsafe {
            let _ = LocalFree(HLOCAL(raw.as_ptr().cast()));
        }
        text.map_err(|err| format!("the SID string was not valid UTF-16: {err}"))
    }

    fn well_known(kind: WELL_KNOWN_SID_TYPE) -> Result<SidBuffer, String> {
        let mut buffer = SidBuffer::zeroed();
        let mut size = SECURITY_MAX_SID_SIZE;
        // SAFETY: the buffer is `SECURITY_MAX_SID_SIZE` bytes, the documented
        // maximum, and `size` says so. A null domain SID is what the
        // machine-local well-known types take.
        unsafe { CreateWellKnownSid(kind, PSID::default(), buffer.psid_mut(), &mut size) }
            .map_err(|err| format!("CreateWellKnownSid failed: {err}"))?;
        Ok(buffer)
    }

    /// The SID this process runs as.
    ///
    /// Not the object's owner: see this module's parent doc. If policy makes
    /// Administrators the owner of what an elevated process creates, an
    /// owner-derived grant locks the user out of their own file.
    fn token_user() -> Result<SidBuffer, String> {
        let mut token = HANDLE::default();
        // SAFETY: `GetCurrentProcess` is a pseudo-handle needing no close, and
        // `token` receives a real handle closed below.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
            .map_err(|err| format!("OpenProcessToken failed: {err}"))?;

        let mut needed = 0_u32;
        // SAFETY: passing no buffer is the documented way to ask for the size.
        // The call is expected to fail with ERROR_INSUFFICIENT_BUFFER; the size
        // is what comes back, and is checked below rather than the status.
        let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };

        let outcome = read_token_user(token, needed);
        // SAFETY: `token` is a real handle from `OpenProcessToken`, used for
        // the last time above.
        unsafe {
            let _ = CloseHandle(token);
        }
        outcome
    }

    #[cfg(test)]
    pub(super) fn token_user_sid_for_test() -> Result<String, String> {
        sid_string(token_user()?.psid())
    }

    fn read_token_user(token: HANDLE, needed: u32) -> Result<SidBuffer, String> {
        if needed == 0 {
            return Err("GetTokenInformation(TokenUser) asked for no buffer".to_owned());
        }
        // `u64` words rather than bytes so the buffer is 8-byte aligned: a
        // `TOKEN_USER` holds a pointer, and the SID the OS writes after it
        // holds 32-bit sub-authorities.
        let mut buffer = vec![0_u64; (needed as usize).div_ceil(8)];
        let mut written = 0_u32;
        // SAFETY: the buffer is at least `needed` bytes and `needed` says so.
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(buffer.as_mut_ptr().cast()),
                needed,
                &mut written,
            )
        }
        .map_err(|err| format!("GetTokenInformation(TokenUser) failed: {err}"))?;

        // SAFETY: the call above filled the buffer with a `TOKEN_USER` followed
        // by the SID it points at, and the buffer is aligned for both.
        let info = unsafe { *buffer.as_ptr().cast::<TOKEN_USER>() };
        if info.User.Sid.is_invalid() {
            return Err("GetTokenInformation(TokenUser) returned no SID".to_owned());
        }

        // Copied out, because `buffer` is about to go and the SID points into
        // it.
        let mut copy = SidBuffer::zeroed();
        // SAFETY: the destination is `SECURITY_MAX_SID_SIZE` bytes and the
        // source is a valid SID for as long as `buffer` lives, which is past
        // this call.
        unsafe { CopySid(SECURITY_MAX_SID_SIZE, copy.psid_mut(), info.User.Sid) }
            .map_err(|err| format!("CopySid failed on the token user: {err}"))?;
        Ok(copy)
    }

    fn grants() -> Result<Grants, String> {
        Ok(Grants {
            sids: [
                token_user()?,
                well_known(WinLocalSystemSid)?,
                well_known(WinBuiltinAdministratorsSid)?,
            ],
        })
    }

    fn requested(grants: &Grants, kind: PathKind) -> Result<Requested, String> {
        let mut allowed = Vec::with_capacity(grants.sids.len());
        for sid in &grants.sids {
            allowed.push(sid_string(sid.psid())?);
        }
        Ok(Requested {
            allowed,
            mask: FILE_ALL_ACCESS.0,
            inheritable: matches!(kind, PathKind::Directory),
        })
    }

    pub(super) fn apply(path: &Path, kind: PathKind) -> Result<(), String> {
        let wide = wide(path);
        let grants = grants()?;
        let inheritance = match kind {
            // A file grants exactly these three and hands nothing down.
            PathKind::File => NO_INHERITANCE,
            // The directory's entries are inheritable so that anything created
            // inside starts with this list rather than the profile's. That is
            // what lets `discovery::write` create its temporary under an
            // already-protected directory: the token's bytes never touch disk
            // under the inherited access list.
            PathKind::Directory => SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        };

        let entries: Vec<EXPLICIT_ACCESS_W> = grants
            .sids
            .iter()
            .map(|sid| EXPLICIT_ACCESS_W {
                grfAccessPermissions: FILE_ALL_ACCESS.0,
                grfAccessMode: SET_ACCESS,
                grfInheritance: inheritance,
                Trustee: TRUSTEE_W {
                    pMultipleTrustee: std::ptr::null_mut(),
                    MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                    TrusteeForm: TRUSTEE_IS_SID,
                    // Not looked up. `TRUSTEE_IS_UNKNOWN` is the documented
                    // value when the form is a SID, and resolving the type
                    // would need a lookup that can fail on a domain-joined
                    // machine for no gain.
                    TrusteeType: TRUSTEE_IS_UNKNOWN,
                    ptstrName: PWSTR(sid.psid().0.cast()),
                },
            })
            .collect();

        let mut acl: *mut ACL = std::ptr::null_mut();
        // `oldacl: None` on purpose. Merging into what is already there would
        // keep every entry the profile directory handed down, which is the
        // whole defect.
        //
        // SAFETY: `entries` outlives the call, as do the SIDs it points at, and
        // `acl` receives a `LocalAlloc`ed list freed below.
        let status = unsafe { SetEntriesInAclW(Some(&entries), None, &mut acl) };
        check(status, "SetEntriesInAclW")?;
        if acl.is_null() {
            return Err("SetEntriesInAclW reported success and produced no access list".to_owned());
        }

        // `PROTECTED_DACL_SECURITY_INFORMATION` is the half that makes this
        // stick: without it the parent's inheritable entries are merged back in
        // the moment anything re-evaluates inheritance.
        //
        // Deliberately *not* `OWNER_SECURITY_INFORMATION` — taking ownership is
        // not this application's to do — and never the SACL, which needs a
        // privilege a normal user does not hold.
        //
        // SAFETY: `wide` is null-terminated and outlives the call; `acl` is the
        // list built above.
        let status = unsafe {
            SetNamedSecurityInfoW(
                PCWSTR(wide.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                PSID::default(),
                PSID::default(),
                Some(acl),
                None,
            )
        };
        // SAFETY: `acl` came from `LocalAlloc` inside `SetEntriesInAclW` and is
        // not used again.
        unsafe {
            let _ = LocalFree(HLOCAL(acl.cast()));
        }
        check(status, "SetNamedSecurityInfoW")
    }

    pub(super) fn verify(path: &Path, kind: PathKind) -> Permissions {
        let requested = match grants().and_then(|grants| requested(&grants, kind)) {
            Ok(requested) => requested,
            Err(reason) => return Permissions::Unknown { reason },
        };
        let facts = match read_back(path) {
            Ok(facts) => facts,
            Err(reason) => return Permissions::Unknown { reason },
        };

        let verdict = super::classify(&facts, &requested);
        if !verdict.is_confirmed() {
            // The trustees are named here and *not* carried in the verdict.
            // `Permissions` crosses to the settings screen and from there into
            // screenshots and bug reports, and a machine's account and group
            // names are not this application's to publish. Whoever is
            // diagnosing this can read the log; a count is all the screen needs
            // to be honest.
            tracing::warn!(
                path = %path.display(),
                verdict = ?verdict,
                owner = %facts.owner,
                protected = ?facts.protected,
                entries = ?facts.dacl,
                "the access list on this path is not the one that was applied"
            );
        }
        verdict
    }

    /// What the operating system says the object's descriptor is.
    ///
    /// Public to the crate so `examples/dacl_probe.rs` can print it: a probe
    /// that reported only the verdict would be no help the day the verdict is
    /// wrong.
    ///
    /// # Errors
    ///
    /// The operating system's own message.
    pub fn read_back(path: &Path) -> Result<DaclFacts, String> {
        let wide = wide(path);
        let mut owner = PSID::default();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // Owner and DACL in one call: the owner is a trustee in its own right,
        // and reading it separately would be two moments describing one object.
        //
        // SAFETY: `wide` is null-terminated and outlives the call. The out
        // pointers all point into `descriptor`, which is freed below after
        // everything has been copied out of it.
        let status = unsafe {
            GetNamedSecurityInfoW(
                PCWSTR(wide.as_ptr()),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                Some(&mut owner),
                None,
                Some(&mut dacl),
                None,
                &mut descriptor,
            )
        };
        check(status, "GetNamedSecurityInfoW")?;

        let facts = collect(owner, dacl, descriptor);
        // SAFETY: `descriptor` came from `LocalAlloc` inside
        // `GetNamedSecurityInfoW`; `facts` owns copies of everything read out
        // of it, and nothing here touches it again.
        unsafe {
            let _ = LocalFree(HLOCAL(descriptor.0));
        }
        facts
    }

    fn collect(
        owner: PSID,
        dacl: *mut ACL,
        descriptor: PSECURITY_DESCRIPTOR,
    ) -> Result<DaclFacts, String> {
        let owner = if owner.is_invalid() {
            String::new()
        } else {
            sid_string(owner)?
        };

        let mut control = 0_u16;
        let mut revision = 0_u32;
        // SAFETY: `descriptor` is the self-relative descriptor the read
        // returned.
        let protected =
            unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) }
                .is_ok()
                .then_some(control & SE_DACL_PROTECTED.0 != 0);

        let entries = if dacl.is_null() {
            // A NULL access list. Not "nothing is granted" — no check at all.
            None
        } else {
            Some(collect_aces(dacl)?)
        };

        Ok(DaclFacts {
            protected,
            owner,
            dacl: entries,
        })
    }

    fn collect_aces(dacl: *mut ACL) -> Result<Vec<AceFacts>, String> {
        // SAFETY: `dacl` is non-null and points into the descriptor the read
        // returned.
        let count = unsafe { (*dacl).AceCount };
        let mut aces = Vec::with_capacity(count as usize);
        for index in 0..u32::from(count) {
            let mut raw: *mut std::ffi::c_void = std::ptr::null_mut();
            // SAFETY: `index` is below the list's own `AceCount`.
            if unsafe { GetAce(dacl, index, &mut raw) }.is_err() || raw.is_null() {
                return Err(format!("GetAce failed for entry {index} of {count}"));
            }
            // SAFETY: every ACE starts with an `ACE_HEADER`, whatever follows.
            let header = unsafe { *raw.cast::<ACE_HEADER>() };
            let flags = u32::from(header.AceFlags);
            let inherited = flags & INHERITED_ACE.0 != 0;
            let inheritable = flags & (OBJECT_INHERIT_ACE.0 | CONTAINER_INHERIT_ACE.0) != 0;
            let ace_type = u32::from(header.AceType);

            if ace_type == ACCESS_ALLOWED_ACE_TYPE || ace_type == ACCESS_DENIED_ACE_TYPE {
                // `ACCESS_ALLOWED_ACE` and `ACCESS_DENIED_ACE` have the same
                // layout: header, mask, then the SID inline from `SidStart`.
                let known = raw.cast::<ACCESS_ALLOWED_ACE>();
                // SAFETY: the type byte says this is that layout.
                let mask = unsafe { (*known).Mask };
                // SAFETY: same. `addr_of_mut!` rather than a reference because
                // the SID overruns the declared `u32`.
                let sid = PSID(unsafe { std::ptr::addr_of_mut!((*known).SidStart) }.cast());
                aces.push(AceFacts {
                    sid: sid_string(sid)?,
                    kind: if ace_type == ACCESS_ALLOWED_ACE_TYPE {
                        AceKind::Allow
                    } else {
                        AceKind::Deny
                    },
                    inherited,
                    inheritable,
                    mask,
                });
            } else {
                aces.push(AceFacts {
                    sid: String::new(),
                    kind: AceKind::Unrecognised {
                        ace_type: header.AceType,
                    },
                    inherited,
                    inheritable,
                    mask: 0,
                });
            }
        }
        Ok(aces)
    }
}

#[cfg(target_os = "windows")]
pub use windows_impl::read_back;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    const USER: &str = "S-1-5-21-2149418485-2517401309-221634420-1001";
    const SYSTEM: &str = "S-1-5-18";
    const ADMINISTRATORS: &str = "S-1-5-32-544";
    const FILE_ALL: u32 = 0x001F_01FF;

    fn requested() -> Requested {
        Requested {
            allowed: vec![
                USER.to_owned(),
                SYSTEM.to_owned(),
                ADMINISTRATORS.to_owned(),
            ],
            mask: FILE_ALL,
            inheritable: false,
        }
    }

    fn allow(sid: &str) -> AceFacts {
        AceFacts {
            sid: sid.to_owned(),
            kind: AceKind::Allow,
            inherited: false,
            inheritable: false,
            mask: FILE_ALL,
        }
    }

    fn inherited(sid: &str) -> AceFacts {
        AceFacts {
            inherited: true,
            ..allow(sid)
        }
    }

    fn facts(owner: &str, dacl: Vec<AceFacts>) -> DaclFacts {
        DaclFacts {
            protected: Some(true),
            owner: owner.to_owned(),
            dacl: Some(dacl),
        }
    }

    #[test]
    fn the_three_expected_trustees_and_nothing_else_is_the_confirmed_state() {
        let read_back = facts(
            USER,
            vec![allow(USER), allow(SYSTEM), allow(ADMINISTRATORS)],
        );
        assert_eq!(
            classify(&read_back, &requested()),
            Permissions::OwnerAndAdministrators
        );
    }

    #[test]
    fn the_dacl_measured_on_the_real_machine_reports_the_extra_trustees() {
        // The exposure this module was written for, from docs/BENCH_CHECK.md
        // finding 2: five entries, every one inherited, the descriptor not
        // protected, and two trustees that are neither the user nor
        // administrators. Written out as strings so the regression test for a
        // Windows defect runs on CI's Linux box.
        let measured = DaclFacts {
            protected: Some(false),
            owner: USER.to_owned(),
            dacl: Some(vec![
                // Jobbdator\CodexSandboxUsers
                inherited("S-1-5-21-2149418485-2517401309-221634420-1002"),
                inherited("S-1-5-21-2149418485-2517401309-221634420-3839493994"),
                inherited(SYSTEM),
                inherited(ADMINISTRATORS),
                inherited(USER),
            ]),
        };
        assert_eq!(
            classify(&measured, &requested()),
            Permissions::ExtraTrustees { count: 2 },
            "two of the five are neither the user, the system nor administrators"
        );
    }

    #[test]
    fn extra_trustees_outrank_inheritance_because_the_grant_is_the_worse_fact() {
        // Both are true of the measured list. The verdict names the one a
        // person has to act on.
        let both = DaclFacts {
            protected: Some(false),
            owner: USER.to_owned(),
            dacl: Some(vec![inherited(USER), inherited("S-1-5-21-9-9-9-1234")]),
        };
        assert_eq!(
            classify(&both, &requested()),
            Permissions::ExtraTrustees { count: 1 }
        );
    }

    #[test]
    fn a_null_access_list_is_not_an_empty_one() {
        // The distinction the `Option` exists for. A NULL DACL grants everyone
        // everything; an empty DACL grants nobody anything. Reporting the first
        // as the second is reporting the worst case as the best.
        let null = DaclFacts {
            protected: Some(true),
            owner: USER.to_owned(),
            dacl: None,
        };
        assert_eq!(classify(&null, &requested()), Permissions::GrantsEveryone);

        let empty = facts(USER, vec![]);
        assert_eq!(classify(&empty, &requested()), Permissions::Incomplete);
    }

    #[test]
    fn an_allow_entry_for_everyone_is_named_as_that_rather_than_counted() {
        for world in ["S-1-1-0", "S-1-5-11", "S-1-5-32-545"] {
            let read_back = facts(
                USER,
                vec![
                    allow(USER),
                    allow(SYSTEM),
                    allow(ADMINISTRATORS),
                    allow(world),
                ],
            );
            assert_eq!(
                classify(&read_back, &requested()),
                Permissions::GrantsEveryone,
                "{world} means everyone, and a count of one would understate it"
            );
        }
    }

    #[test]
    fn an_owner_outside_the_three_counts_as_a_trustee_without_an_entry_of_its_own() {
        // An owner always holds WRITE_DAC and can re-grant itself, so a list
        // that looks perfect under a foreign owner is not a restriction.
        let read_back = facts(
            "S-1-5-21-9-9-9-500",
            vec![allow(USER), allow(SYSTEM), allow(ADMINISTRATORS)],
        );
        assert_eq!(
            classify(&read_back, &requested()),
            Permissions::ExtraTrustees { count: 1 }
        );
    }

    #[test]
    fn a_deny_entry_for_an_outsider_is_not_a_trustee_that_can_read_it() {
        let read_back = facts(
            USER,
            vec![
                AceFacts {
                    kind: AceKind::Deny,
                    ..allow("S-1-5-21-9-9-9-1234")
                },
                allow(USER),
                allow(SYSTEM),
                allow(ADMINISTRATORS),
            ],
        );
        assert_eq!(
            classify(&read_back, &requested()),
            Permissions::OwnerAndAdministrators
        );
    }

    #[test]
    fn an_entry_this_code_cannot_read_is_counted_rather_than_waved_through() {
        let read_back = facts(
            USER,
            vec![
                allow(USER),
                allow(SYSTEM),
                allow(ADMINISTRATORS),
                AceFacts {
                    sid: String::new(),
                    kind: AceKind::Unrecognised { ace_type: 5 },
                    ..allow(USER)
                },
            ],
        );
        assert_eq!(
            classify(&read_back, &requested()),
            Permissions::ExtraTrustees { count: 1 },
            "not understanding an entry must never read as the entry being fine"
        );
    }

    #[test]
    fn an_unreadable_protected_flag_falls_back_to_the_entries_own_inheritance() {
        // `GetSecurityDescriptorControl` reporting nothing is not the same as
        // it reporting "not protected". With `None`, the entries decide.
        let unknown_control = DaclFacts {
            protected: None,
            ..facts(
                USER,
                vec![allow(USER), allow(SYSTEM), allow(ADMINISTRATORS)],
            )
        };
        assert_eq!(
            classify(&unknown_control, &requested()),
            Permissions::OwnerAndAdministrators
        );

        let unknown_but_inherited = DaclFacts {
            protected: None,
            ..facts(
                USER,
                vec![inherited(USER), allow(SYSTEM), allow(ADMINISTRATORS)],
            )
        };
        assert_eq!(
            classify(&unknown_but_inherited, &requested()),
            Permissions::StillInheriting
        );
    }

    #[test]
    fn the_right_three_arriving_by_inheritance_is_still_reported() {
        let read_back = DaclFacts {
            protected: Some(false),
            ..facts(
                USER,
                vec![allow(USER), allow(SYSTEM), allow(ADMINISTRATORS)],
            )
        };
        assert_eq!(
            classify(&read_back, &requested()),
            Permissions::StillInheriting,
            "what a directory grants today it can stop granting tomorrow"
        );
    }

    #[test]
    fn a_missing_or_reduced_grant_is_incomplete_rather_than_confirmed() {
        let missing = facts(USER, vec![allow(USER), allow(SYSTEM)]);
        assert_eq!(classify(&missing, &requested()), Permissions::Incomplete);

        let reduced = facts(
            USER,
            vec![
                AceFacts {
                    mask: 0x0001,
                    ..allow(USER)
                },
                allow(SYSTEM),
                allow(ADMINISTRATORS),
            ],
        );
        assert_eq!(classify(&reduced, &requested()), Permissions::Incomplete);

        let denied = facts(
            USER,
            vec![
                allow(USER),
                AceFacts {
                    kind: AceKind::Deny,
                    ..allow(USER)
                },
                allow(SYSTEM),
                allow(ADMINISTRATORS),
            ],
        );
        assert_eq!(classify(&denied, &requested()), Permissions::Incomplete);
    }

    #[test]
    fn a_directory_wants_its_entries_inheritable_and_a_file_does_not() {
        // The property `discovery::write` relies on: a temporary created inside
        // an already-protected directory starts protected, so the token's bytes
        // never exist under the inherited list.
        let wanted = Requested {
            inheritable: true,
            ..requested()
        };
        let not_handed_down = facts(
            USER,
            vec![allow(USER), allow(SYSTEM), allow(ADMINISTRATORS)],
        );
        assert_eq!(classify(&not_handed_down, &wanted), Permissions::Incomplete);

        let handed_down = facts(
            USER,
            vec![
                AceFacts {
                    inheritable: true,
                    ..allow(USER)
                },
                AceFacts {
                    inheritable: true,
                    ..allow(SYSTEM)
                },
                AceFacts {
                    inheritable: true,
                    ..allow(ADMINISTRATORS)
                },
            ],
        );
        assert_eq!(
            classify(&handed_down, &wanted),
            Permissions::OwnerAndAdministrators
        );
    }

    #[test]
    fn a_perfect_file_in_a_loose_directory_is_not_a_restricted_file() {
        // It can be deleted and replaced, and whatever reads it next reads the
        // replacement. For a file whose job is to carry a token, that is worse
        // than being readable.
        assert_eq!(
            combine(
                &Permissions::ExtraTrustees { count: 2 },
                &Permissions::OwnerAndAdministrators
            ),
            Permissions::DirectoryNotRestricted
        );
        assert_eq!(
            combine(&Permissions::OwnerOnly, &Permissions::OwnerOnly),
            Permissions::OwnerOnly
        );
    }

    #[test]
    fn the_files_own_problem_wins_over_the_directorys() {
        // A caller shown "the directory is loose" would go and fix the wrong
        // thing.
        assert_eq!(
            combine(
                &Permissions::StillInheriting,
                &Permissions::ExtraTrustees { count: 3 }
            ),
            Permissions::ExtraTrustees { count: 3 }
        );
    }

    #[test]
    fn only_a_read_back_restriction_is_confirmed() {
        assert!(Permissions::OwnerOnly.is_confirmed());
        assert!(Permissions::OwnerAndAdministrators.is_confirmed());
        // Not because it is necessarily bad -- because nothing looked.
        assert!(!Permissions::InheritedFromProfile.is_confirmed());
        assert!(!Permissions::Wider { mode: 0o644 }.is_confirmed());
        assert!(!Permissions::ExtraTrustees { count: 1 }.is_confirmed());
        assert!(!Permissions::StillInheriting.is_confirmed());
        assert!(!Permissions::GrantsEveryone.is_confirmed());
        assert!(!Permissions::Incomplete.is_confirmed());
        assert!(!Permissions::DirectoryNotRestricted.is_confirmed());
        assert!(!Permissions::Unknown {
            reason: "no".to_owned()
        }
        .is_confirmed());
    }

    #[test]
    fn the_windows_confirmed_state_never_claims_owner_only() {
        // The specific thing this must not do is overstate the protection. An
        // administrator really can read it, and the sentence says so.
        let explanation = Permissions::OwnerAndAdministrators.explain();
        assert!(!explanation.contains("only by your user"), "{explanation}");
        assert!(explanation.contains("administrators"), "{explanation}");
    }

    #[test]
    fn no_explanation_mentions_the_caller_that_happens_to_be_the_local_api() {
        // Two crates report through this vocabulary and one of them describes a
        // PID record. A sentence about "the local API" would be false there.
        for permissions in [
            Permissions::OwnerOnly,
            Permissions::OwnerAndAdministrators,
            Permissions::InheritedFromProfile,
            Permissions::Wider { mode: 0o644 },
            Permissions::ExtraTrustees { count: 1 },
            Permissions::StillInheriting,
            Permissions::GrantsEveryone,
            Permissions::Incomplete,
            Permissions::DirectoryNotRestricted,
            Permissions::Unknown {
                reason: "no".to_owned(),
            },
        ] {
            let explanation = permissions.explain();
            assert!(
                !explanation.to_lowercase().contains("api"),
                "{permissions:?}: {explanation}"
            );
        }
    }

    #[test]
    fn applying_and_verifying_agree_on_a_real_file() {
        // The end-to-end shape, on whichever platform this is. Not a claim
        // about which state is right -- that differs by platform -- only that a
        // freshly protected file reads back as confirmed.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token.json");
        std::fs::write(&path, "{}").unwrap();

        apply(dir.path(), PathKind::Directory).unwrap();
        apply(&path, PathKind::File).unwrap();

        let file = verify(&path, PathKind::File);
        let directory = verify(dir.path(), PathKind::Directory);
        if cfg!(any(unix, target_os = "windows")) {
            assert!(file.is_confirmed(), "file: {file:?}");
            assert!(directory.is_confirmed(), "directory: {directory:?}");
            assert!(combine(&directory, &file).is_confirmed());
        } else {
            assert_eq!(file, Permissions::InheritedFromProfile);
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_protected_dacl_survives_a_rename_within_the_same_directory() {
        // The property `discovery::write` depends on and never measured: it
        // restricts a temporary and then renames it into place. A move within
        // one directory keeps the file object and its descriptor; a copy or a
        // cross-volume move would not. If that is ever untrue, this fails
        // rather than the application quietly reporting a DACL the final file
        // does not have.
        let dir = tempfile::tempdir().unwrap();
        apply(dir.path(), PathKind::Directory).unwrap();

        let temporary = dir.path().join("local-api.json.new");
        std::fs::write(&temporary, "{}").unwrap();
        apply(&temporary, PathKind::File).unwrap();
        let before = verify(&temporary, PathKind::File);
        assert_eq!(before, Permissions::OwnerAndAdministrators);

        let final_path = dir.path().join("local-api.json");
        std::fs::rename(&temporary, &final_path).unwrap();

        assert_eq!(
            verify(&final_path, PathKind::File),
            Permissions::OwnerAndAdministrators,
            "the descriptor did not survive the rename"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_file_created_inside_a_protected_directory_inherits_only_the_three() {
        // Why `restrict_directory` runs before the temporary is written. If the
        // inheritable entries did not take, the token's bytes would exist under
        // the profile's access list for the moment between write and restrict.
        //
        // Measured: such a file gets exactly the three, but *not*
        // `SE_DACL_PROTECTED` -- it is inheriting them, so it would follow the
        // directory if the directory changed. That is why `apply` still runs on
        // the file rather than the directory being trusted to have covered it.
        let dir = tempfile::tempdir().unwrap();
        apply(dir.path(), PathKind::Directory).unwrap();

        let path = dir.path().join("inherited.json");
        std::fs::write(&path, "{}").unwrap();

        let facts = read_back(&path).expect("the file was just written");
        let entries = facts.dacl.expect("a real file has an access list");
        let requested = Requested {
            // Elevated Windows can assign Administrators as the owner. The
            // directory grants the process token user, independently of owner.
            allowed: vec![
                windows_impl::token_user_sid_for_test().expect("the process has a token user"),
                "S-1-5-18".to_owned(),
            ],
            mask: 0,
            inheritable: false,
        };
        let unexpected: Vec<&AceFacts> = entries
            .iter()
            .filter(|ace| {
                ace.kind == AceKind::Allow
                    && !requested.allowed.iter().any(|sid| sid == &ace.sid)
                    && ace.sid != "S-1-5-32-544"
            })
            .collect();
        assert!(
            unexpected.is_empty(),
            "a file created under a protected directory inherited {unexpected:?}"
        );
    }
}
