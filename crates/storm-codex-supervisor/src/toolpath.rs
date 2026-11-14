//! Paths handed to the Postgres tools.
//!
//! Postgres for Windows is a narrow-character program: `main(argc, argv)` receives its
//! arguments converted to the system's ANSI code page, and it finds its own `share/` folder
//! through `GetModuleFileNameA`. A character outside that code page (a Cyrillic Windows user
//! name on a Western Windows, for example) becomes `?`, so `initdb -D <data>` or even the tool's
//! own path points nowhere. Rust starts the process with the full Unicode path, but every path
//! the tool reads from its arguments or derives from its location must survive the code page.
//!
//! [`ToolPaths::map`] returns, for an absolute path, a form that does:
//! 1. the path itself when every character fits the ANSI code page (the usual case);
//! 2. otherwise its **8.3 short form** (`GetShortPathNameW` on the deepest existing ancestor,
//!    the missing tail appended as is), when that fits;
//! 3. otherwise (8.3 names disabled on the volume, or created after the folder) an **alias**:
//!    a directory junction `<alias base>\<hash>` pointing at the deepest folder of the path whose
//!    name does not fit, followed by the rest of the path. The junction holds no data; the files
//!    stay where they are, so the data root, exports, imports, the uninstaller's "delete data"
//!    option and a later switch between the three forms all see the same files.
//!
//! The default alias base is `%ProgramData%\StormCodex-<user SID>`: outside the user profile
//! (whose path is the one that does not fit), created by this process with a **protected DACL**
//! (only the user and SYSTEM) and refused when it exists with another owner or an inherited
//! DACL, so no other local account can create or redirect a junction in it.

use std::io;
use std::path::{Component, Path, PathBuf};

/// Why a path could not be given a form the Postgres tools can read.
#[derive(Debug, thiserror::Error)]
pub enum ToolPathError {
    #[error(
        "the folder {path} cannot be passed to Postgres (its name does not fit the Windows \
         ANSI code page) and no alias could be made: {reason}"
    )]
    NoAlias { path: PathBuf, reason: String },
    #[error(
        "the alias folder {0} already exists but is not this user's private folder; \
         remove it (or ask an administrator to) and start Storm Codex again"
    )]
    UnsafeAliasBase(PathBuf),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// How [`ToolPaths::map`] may rewrite a path (see the module documentation).
#[derive(Debug, Clone)]
pub struct ToolPaths {
    short_names: bool,
    /// `None` = the default `%ProgramData%\StormCodex-<SID>`, resolved when first needed.
    alias_base: Option<PathBuf>,
}

impl Default for ToolPaths {
    fn default() -> Self {
        Self::system()
    }
}

impl ToolPaths {
    /// Short names first, then the default alias base.
    pub fn system() -> Self {
        Self {
            short_names: true,
            alias_base: None,
        }
    }

    /// Explicit settings, for tests and diagnosis: `short_names: false` forces the alias for
    /// any path that does not fit; `alias_base` replaces `%ProgramData%\StormCodex-<SID>` (it
    /// gets the same private-folder checks).
    pub fn new(short_names: bool, alias_base: PathBuf) -> Self {
        Self {
            short_names,
            alias_base: Some(alias_base),
        }
    }

    /// A form of `path` every character of which fits the ANSI code page, naming the same file.
    pub fn map(&self, path: &Path) -> Result<PathBuf, ToolPathError> {
        let path = std::path::absolute(path).map_err(|source| ToolPathError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if fits_ansi(&path) {
            return Ok(path);
        }
        if self.short_names {
            if let Some(short) = short_form(&path) {
                if fits_ansi(&short) {
                    return Ok(short);
                }
            }
        }
        self.alias(&path)
    }

    fn alias(&self, path: &Path) -> Result<PathBuf, ToolPathError> {
        let no_alias = |reason: String| ToolPathError::NoAlias {
            path: path.to_path_buf(),
            reason,
        };
        // Anchor = the path up to its last component that does not fit; the rest fits.
        let parts: Vec<Component<'_>> = path.components().collect();
        let last_bad = parts
            .iter()
            .rposition(|c| !fits_ansi(Path::new(c.as_os_str())))
            .ok_or_else(|| no_alias("no component needs an alias".into()))?;
        let anchor: PathBuf = parts[..=last_bad].iter().collect();
        let rest: PathBuf = parts[last_bad + 1..].iter().collect();
        if !anchor.is_dir() {
            return Err(no_alias(format!("{} is not a folder", anchor.display())));
        }
        let base = match &self.alias_base {
            Some(base) => base.clone(),
            None => default_alias_base().map_err(no_alias)?,
        };
        if !fits_ansi(&base) {
            return Err(no_alias(format!(
                "the alias folder {} does not fit either",
                base.display()
            )));
        }
        ensure_private_dir(&base)?;
        let link = base.join(format!("{:016x}", path_hash(&anchor)));
        ensure_junction(&link, &anchor)?;
        Ok(link.join(rest))
    }
}

/// FNV-1a over the lower-cased path: stable across builds (unlike `DefaultHasher`), so the same
/// folder always gets the same alias.
fn path_hash(path: &Path) -> u64 {
    let text = path.to_string_lossy().to_lowercase();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The deepest existing ancestor of `path` (itself included) in 8.3 form, plus the missing tail.
fn short_form(path: &Path) -> Option<PathBuf> {
    let mut existing = path.to_path_buf();
    let mut tail = Vec::new();
    while !existing.exists() {
        tail.push(existing.file_name()?.to_os_string());
        existing = existing.parent()?.to_path_buf();
    }
    let mut short = sys::short_path(&existing)?;
    for part in tail.iter().rev() {
        short.push(part);
    }
    Some(short)
}

/// Whether every character of `path` survives a round trip through the ANSI code page.
pub(crate) fn fits_ansi(path: &Path) -> bool {
    sys::fits_ansi(path)
}

fn default_alias_base() -> Result<PathBuf, String> {
    let program_data = std::env::var_os("ProgramData")
        .or_else(|| std::env::var_os("ALLUSERSPROFILE"))
        .ok_or_else(|| "%ProgramData% is not set".to_string())?;
    let sid = sys::user_sid()?;
    Ok(PathBuf::from(program_data).join(format!("StormCodex-{sid}")))
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> ToolPathError + '_ {
    move |source| ToolPathError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Creates `dir` with a protected DACL (this user and SYSTEM only), or checks that an existing
/// one is such a folder owned by this user.
fn ensure_private_dir(dir: &Path) -> Result<(), ToolPathError> {
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    match sys::create_private_dir(dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let meta = std::fs::symlink_metadata(dir).map_err(io_err(dir))?;
            if meta.is_dir() && !meta.file_type().is_symlink() && sys::is_private_dir(dir) {
                Ok(())
            } else {
                Err(ToolPathError::UnsafeAliasBase(dir.to_path_buf()))
            }
        }
        Err(e) => Err(io_err(dir)(e)),
    }
}

/// Makes `link` a junction to `target` (replacing a junction that points elsewhere).
fn ensure_junction(link: &Path, target: &Path) -> Result<(), ToolPathError> {
    if std::fs::symlink_metadata(link).is_ok() {
        if same_target(link, target) {
            return Ok(());
        }
        // Ours to replace: only this user can write in the private alias folder.
        std::fs::remove_dir(link)
            .or_else(|_| std::fs::remove_file(link))
            .map_err(io_err(link))?;
    }
    sys::create_junction(link, target).map_err(io_err(link))?;
    if same_target(link, target) {
        Ok(())
    } else {
        Err(ToolPathError::NoAlias {
            path: target.to_path_buf(),
            reason: format!("{} does not lead to it", link.display()),
        })
    }
}

fn same_target(link: &Path, target: &Path) -> bool {
    match (std::fs::canonicalize(link), std::fs::canonicalize(target)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod sys {
    //! Win32 calls (`windows-sys`). Every buffer is sized by the API's own length query.

    use std::ffi::OsString;
    use std::io;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{
        CloseHandle, LocalFree, ERROR_ALREADY_EXISTS, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Globalization::{
        GetACP, WideCharToMultiByte, CP_ACP, WC_NO_BEST_FIT_CHARS,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        GetSecurityDescriptorControl, GetTokenInformation, TokenUser, DACL_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
        SE_DACL_PROTECTED, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateDirectoryW, CreateFileW, GetShortPathNameW, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::System::IO::DeviceIoControl;

    /// `FSCTL_SET_REPARSE_POINT` (winioctl.h).
    const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
    /// `IO_REPARSE_TAG_MOUNT_POINT` (winnt.h): a directory junction.
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
    /// UTF-8 as the ANSI code page (the "Beta: Use Unicode UTF-8" option): everything fits.
    const CP_UTF8: u32 = 65001;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn fits_ansi(path: &Path) -> bool {
        // SAFETY: plain query, no pointers.
        if unsafe { GetACP() } == CP_UTF8 {
            return true;
        }
        let text: Vec<u16> = path.as_os_str().encode_wide().collect();
        if text.is_empty() {
            return true;
        }
        let Ok(len) = i32::try_from(text.len()) else {
            return false;
        };
        let mut used_default = 0;
        // SAFETY: `text` is valid for `len` UTF-16 units; a null output buffer of size 0 asks
        // for the size only; `used_default` is a valid BOOL out-pointer (allowed for CP_ACP).
        let n = unsafe {
            WideCharToMultiByte(
                CP_ACP,
                WC_NO_BEST_FIT_CHARS,
                text.as_ptr(),
                len,
                null_mut(),
                0,
                null(),
                &mut used_default,
            )
        };
        n > 0 && used_default == 0
    }

    pub fn short_path(path: &Path) -> Option<PathBuf> {
        let long = wide(path.as_os_str());
        // SAFETY: `long` is NUL-terminated; a null buffer of size 0 returns the needed size.
        let needed = unsafe { GetShortPathNameW(long.as_ptr(), null_mut(), 0) };
        if needed == 0 {
            return None;
        }
        let mut buf = vec![0u16; needed as usize];
        // SAFETY: `buf` holds `needed` units, as reported above.
        let n = unsafe { GetShortPathNameW(long.as_ptr(), buf.as_mut_ptr(), needed) };
        if n == 0 || n >= needed {
            return None;
        }
        buf.truncate(n as usize);
        Some(PathBuf::from(OsString::from_wide(&buf)))
    }

    /// Closes a handle on drop.
    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle was returned open by the API and is closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// Frees a `LocalAlloc`ed pointer on drop.
    struct Local(*mut core::ffi::c_void);
    impl Drop for Local {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: allocated by the API with LocalAlloc, freed once.
                unsafe { LocalFree(self.0) };
            }
        }
    }

    /// The current user's SID (token user) in string form (`S-1-5-21-…`).
    fn token_user_sid() -> io::Result<String> {
        let mut token: HANDLE = null_mut();
        // SAFETY: pseudo-handle of the current process; `token` is a valid out-pointer.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = Handle(token);
        let mut len = 0u32;
        // SAFETY: size query with a null buffer.
        unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut len) };
        if len == 0 {
            return Err(io::Error::last_os_error());
        }
        // u64 storage keeps the TOKEN_USER header aligned.
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        // SAFETY: `buf` holds at least `len` bytes.
        if unsafe {
            GetTokenInformation(token.0, TokenUser, buf.as_mut_ptr().cast(), len, &mut len)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the API filled a TOKEN_USER at the start of `buf`; its SID points inside it.
        let sid = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        sid_string(sid)
    }

    fn sid_string(sid: PSID) -> io::Result<String> {
        let mut out: *mut u16 = null_mut();
        // SAFETY: `sid` is a valid SID; `out` receives a LocalAlloc'ed NUL-terminated string.
        if unsafe { ConvertSidToStringSidW(sid, &mut out) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let owned = Local(out.cast());
        let mut len = 0;
        // SAFETY: NUL-terminated string from the API.
        while unsafe { *out.add(len) } != 0 {
            len += 1;
        }
        // SAFETY: `len` units before the NUL are initialised.
        let units = unsafe { std::slice::from_raw_parts(out, len) };
        let text = String::from_utf16_lossy(units);
        drop(owned);
        Ok(text)
    }

    pub fn user_sid() -> Result<String, String> {
        token_user_sid().map_err(|e| format!("cannot read the user's SID: {e}"))
    }

    pub fn create_private_dir(dir: &Path) -> io::Result<()> {
        let sid = user_sid().map_err(io::Error::other)?;
        // Owner = the user; protected DACL: full control for the user and SYSTEM only,
        // inherited by everything created inside.
        let sddl = format!("O:{sid}D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)");
        let sddl_w = wide(std::ffi::OsStr::new(&sddl));
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: NUL-terminated SDDL; `sd` receives a LocalAlloc'ed descriptor.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_w.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let _sd = Local(sd);
        let attrs = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };
        let path = wide(dir.as_os_str());
        // SAFETY: NUL-terminated path, valid attributes for the call's duration.
        if unsafe { CreateDirectoryW(path.as_ptr(), &attrs) } == 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(ERROR_ALREADY_EXISTS as i32) {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, err));
            }
            return Err(err);
        }
        Ok(())
    }

    /// Owned by the current user, with a protected (not inherited) DACL.
    pub fn is_private_dir(dir: &Path) -> bool {
        let Ok(sid) = user_sid() else { return false };
        let path = wide(dir.as_os_str());
        let mut owner: PSID = null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: NUL-terminated path; out-pointers valid; `sd` is LocalAlloc'ed by the API
        // and `owner` points inside it.
        let rc = unsafe {
            GetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                null_mut(),
                null_mut(),
                &mut sd,
            )
        };
        if rc != 0 {
            return false;
        }
        let _sd = Local(sd);
        let Ok(owner_text) = sid_string(owner) else {
            return false;
        };
        let mut control = 0u16;
        let mut revision = 0u32;
        // SAFETY: valid descriptor from the API; out-pointers valid.
        let ok = unsafe { GetSecurityDescriptorControl(sd, &mut control, &mut revision) } != 0;
        // String SIDs are canonical: comparing them is comparing the SIDs.
        ok && owner_text == sid && control & SE_DACL_PROTECTED != 0
    }

    /// Creates `link` as an empty directory turned into a junction to `target` (absolute).
    pub fn create_junction(link: &Path, target: &Path) -> io::Result<()> {
        std::fs::create_dir(link)?;
        let result = set_mount_point(link, target);
        if result.is_err() {
            let _ = std::fs::remove_dir(link);
        }
        result
    }

    fn set_mount_point(link: &Path, target: &Path) -> io::Result<()> {
        let target = std::path::absolute(target)?;
        let print: Vec<u16> = target.as_os_str().encode_wide().collect();
        let mut substitute: Vec<u16> = r"\??\".encode_utf16().collect();
        substitute.extend_from_slice(&print);
        let to_u16 = |n: usize| {
            u16::try_from(n).map_err(|_| io::Error::other("junction target path too long"))
        };
        let sub_bytes = to_u16(substitute.len() * 2)?;
        let print_bytes = to_u16(print.len() * 2)?;
        // PathBuffer = substitute NUL print NUL.
        let path_buffer_bytes = to_u16((substitute.len() + 1 + print.len() + 1) * 2)?;
        let data_len = to_u16(8 + usize::from(path_buffer_bytes))?;
        let mut buf: Vec<u8> = Vec::with_capacity(8 + usize::from(data_len));
        buf.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        buf.extend_from_slice(&data_len.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // Reserved
        buf.extend_from_slice(&0u16.to_le_bytes()); // SubstituteNameOffset
        buf.extend_from_slice(&sub_bytes.to_le_bytes());
        buf.extend_from_slice(&(sub_bytes + 2).to_le_bytes()); // PrintNameOffset
        buf.extend_from_slice(&print_bytes.to_le_bytes());
        for unit in substitute
            .iter()
            .chain(std::iter::once(&0))
            .chain(print.iter())
            .chain(std::iter::once(&0))
        {
            buf.extend_from_slice(&unit.to_le_bytes());
        }
        let path = wide(link.as_os_str());
        // SAFETY: NUL-terminated path; no security attributes; no template.
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let handle = Handle(handle);
        let len = u32::try_from(buf.len()).map_err(io::Error::other)?;
        let mut returned = 0u32;
        // SAFETY: `buf` is a complete REPARSE_DATA_BUFFER of `len` bytes; no output buffer.
        if unsafe {
            DeviceIoControl(
                handle.0,
                FSCTL_SET_REPARSE_POINT,
                buf.as_ptr().cast(),
                len,
                null_mut(),
                0,
                &mut returned,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod sys {
    //! Elsewhere the tools take paths as bytes: nothing to rewrite.

    use std::io;
    use std::path::{Path, PathBuf};

    pub fn fits_ansi(_path: &Path) -> bool {
        true
    }
    pub fn short_path(_path: &Path) -> Option<PathBuf> {
        None
    }
    pub fn user_sid() -> Result<String, String> {
        Err("no SID outside Windows".into())
    }
    pub fn create_private_dir(dir: &Path) -> io::Result<()> {
        std::fs::create_dir(dir)
    }
    pub fn is_private_dir(_dir: &Path) -> bool {
        false
    }
    pub fn create_junction(link: &Path, target: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }
}

#[cfg(all(test, windows))]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const CYRILLIC: &str = "Пользователь";

    /// The tests need a code page that cannot hold Cyrillic (true on Western Windows and CI).
    fn cyrillic_does_not_fit() -> bool {
        let fits = fits_ansi(Path::new(CYRILLIC));
        if fits {
            eprintln!("this machine's ANSI code page holds Cyrillic: test skipped");
        }
        !fits
    }

    #[test]
    fn ascii_and_accented_paths_are_unchanged() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tp = ToolPaths::new(true, tmp.path().join("alias"));
        let plain = tmp.path().join("Zoë Müller").join("pg");
        assert_eq!(tp.map(&plain).expect("map"), plain);
        assert!(!tmp.path().join("alias").exists(), "no alias made");
    }

    #[test]
    fn a_path_outside_the_code_page_maps_to_one_that_fits_and_names_the_same_folder() {
        if !cyrillic_does_not_fit() {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join(CYRILLIC).join("data");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let tp = ToolPaths::new(true, tmp.path().join("alias"));
        let mapped = tp.map(&dir.join("pg")).expect("map");
        assert!(fits_ansi(&mapped), "{mapped:?}");
        assert!(mapped.ends_with("pg"), "missing tail kept: {mapped:?}");
        assert_eq!(
            std::fs::canonicalize(mapped.parent().expect("parent")).expect("canon mapped"),
            std::fs::canonicalize(&dir).expect("canon dir")
        );
    }

    #[test]
    fn without_short_names_a_private_junction_is_made_once_and_reused() {
        if !cyrillic_does_not_fit() {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let profile = tmp.path().join(CYRILLIC);
        let data = profile.join("AppData").join("data");
        std::fs::create_dir_all(&data).expect("mkdir");
        std::fs::write(data.join("marker"), b"x").expect("write");
        let base = tmp.path().join("alias");
        let tp = ToolPaths::new(false, base.clone());

        let mapped = tp.map(&data.join("marker")).expect("map");
        assert!(fits_ansi(&mapped), "{mapped:?}");
        assert!(mapped.starts_with(&base), "{mapped:?}");
        assert_eq!(std::fs::read(&mapped).expect("read through alias"), b"x");
        // anchored at the folder that does not fit: the rest of the path is kept
        assert!(mapped.ends_with(Path::new("AppData").join("data").join("marker")));
        assert!(
            sys::is_private_dir(&base),
            "alias base has a protected DACL"
        );
        assert_eq!(tp.map(&data.join("marker")).expect("again"), mapped);
        // another file under the same folder uses the same junction
        let other = tp.map(&profile.join("Programs")).expect("other");
        assert_eq!(other.parent(), mapped.ancestors().nth(3));
    }

    #[test]
    fn a_junction_pointing_elsewhere_is_replaced() {
        if !cyrillic_does_not_fit() {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join(CYRILLIC);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).expect("mkdir");
        let base = tmp.path().join("alias");
        let tp = ToolPaths::new(false, base.clone());
        let mapped = tp.map(&dir).expect("map");
        std::fs::remove_dir(&mapped).expect("remove junction");
        sys::create_junction(&mapped, &elsewhere).expect("junction elsewhere");
        assert_eq!(tp.map(&dir).expect("map again"), mapped);
        assert_eq!(
            std::fs::canonicalize(&mapped).expect("canon"),
            std::fs::canonicalize(&dir).expect("canon dir")
        );
    }

    #[test]
    fn an_alias_base_that_is_not_private_is_refused() {
        if !cyrillic_does_not_fit() {
            return;
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join(CYRILLIC);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let base = tmp.path().join("alias");
        std::fs::create_dir_all(&base).expect("plain dir, inherited DACL");
        let tp = ToolPaths::new(false, base.clone());
        match tp.map(&dir) {
            Err(ToolPathError::UnsafeAliasBase(p)) => assert_eq!(p, base),
            other => panic!("expected UnsafeAliasBase, got {other:?}"),
        }
    }

    #[test]
    fn path_hash_is_stable_and_case_insensitive() {
        assert_eq!(
            path_hash(Path::new(r"C:\Users\Пользователь")),
            path_hash(Path::new(r"c:\users\ПОЛЬЗОВАТЕЛЬ"))
        );
        assert_eq!(path_hash(Path::new("")), 0xcbf2_9ce4_8422_2325);
    }
}
