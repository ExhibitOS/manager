// SPDX-License-Identifier: Apache-2.0
//! Offline verifier; does not install, persist trust or mutate engine/data.
use exhibitos_lifecycle::signed_release::{self, Policy};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::Path,
};
fn open(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let f = options.open(path).map_err(|_| "UPDATE_INPUT_UNAVAILABLE")?;
    let m = f.metadata().map_err(|_| "UPDATE_INPUT_UNAVAILABLE")?;
    if !m.is_file() {
        return Err("UPDATE_INPUT_INVALID".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.nlink() != 1 {
            return Err("UPDATE_INPUT_INVALID".into());
        }
    }
    Ok(f)
}
fn bounded(path: &Path, limit: u64, policy: bool) -> Result<Vec<u8>, String> {
    let mut f = open(path)?;
    let m = f.metadata().map_err(|_| "UPDATE_INPUT_INVALID")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if policy && (m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o022 != 0) {
            return Err("UPDATE_POLICY_UNTRUSTED".into());
        }
    }
    #[cfg(not(unix))]
    if policy {
        return Err("UPDATE_POLICY_PLATFORM_UNVERIFIED".into());
    }
    if m.len() > limit {
        return Err("UPDATE_INPUT_LIMIT".into());
    }
    let mut b = Vec::new();
    Read::by_ref(&mut f)
        .take(limit + 1)
        .read_to_end(&mut b)
        .map_err(|_| "UPDATE_INPUT_UNAVAILABLE")?;
    if b.len() as u64 > limit {
        return Err("UPDATE_INPUT_LIMIT".into());
    }
    let after = f.metadata().map_err(|_| "UPDATE_INPUT_INVALID")?;
    let current = std::fs::symlink_metadata(path).map_err(|_| "UPDATE_INPUT_INVALID")?;
    if b.len() as u64 != m.len()
        || current.is_symlink()
        || !same(&m, &after)
        || !same(&after, &current)
    {
        return Err("UPDATE_INPUT_CHANGED".into());
    }
    Ok(b)
}
fn same(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (
            a.dev(),
            a.ino(),
            a.len(),
            a.mtime(),
            a.mtime_nsec(),
            a.ctime(),
            a.ctime_nsec(),
            a.nlink(),
        ) == (
            b.dev(),
            b.ino(),
            b.len(),
            b.mtime(),
            b.mtime_nsec(),
            b.ctime(),
            b.ctime_nsec(),
            b.nlink(),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        false
    }
}
fn main() {
    if let Err(code) = verify() {
        println!("{}", serde_json::json!({"code":code}));
        std::process::exit(1);
    }
}
fn verify() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 8
        || a[1] != "verify"
        || a[2] != "--policy"
        || a[4] != "--release"
        || a[6] != "--artifact"
    {
        return Err("UPDATE_USAGE: exhibitos-update verify --policy <trusted policy> --release <signed envelope> --artifact <local artifact>".into());
    }
    let policy: Policy = serde_json::from_slice(&bounded(
        Path::new(&a[3]),
        signed_release::MAX_PAYLOAD as u64,
        true,
    )?)
    .map_err(|_| "UPDATE_POLICY_INVALID")?;
    let envelope = bounded(Path::new(&a[5]), signed_release::MAX_ENVELOPE as u64, false)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "UPDATE_CLOCK_UNVERIFIED")?
        .as_secs();
    let mut verified =
        signed_release::verify(&envelope, &policy, now).map_err(|e| e.code().to_string())?;
    let path = Path::new(&a[7]);
    if path.file_name().and_then(|n| n.to_str()) != Some(verified.release().artifact.name.as_str())
    {
        return Err("UPDATE_ARTIFACT_NAME_MISMATCH".into());
    }
    let mut artifact = open(path)?;
    let before = artifact.metadata().map_err(|_| "UPDATE_INPUT_INVALID")?;
    verified
        .verify_artifact(&mut artifact)
        .map_err(|e| e.code().to_string())?;
    let after = artifact.metadata().map_err(|_| "UPDATE_INPUT_INVALID")?;
    let current = std::fs::symlink_metadata(path).map_err(|_| "UPDATE_INPUT_INVALID")?;
    if current.is_symlink() || !same(&before, &after) || !same(&after, &current) {
        return Err("UPDATE_ARTIFACT_CHANGED".into());
    }
    let finished = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "UPDATE_CLOCK_UNVERIFIED")?
        .as_secs();
    signed_release::verify(&envelope, &policy, finished).map_err(|e| e.code().to_string())?;
    println!(
        "{}",
        serde_json::to_string(&verified.receipt()).map_err(|_| "UPDATE_RECEIPT_INVALID")?
    );
    Ok(())
}
