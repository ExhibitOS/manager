// SPDX-License-Identifier: Apache-2.0
//! Fixed Hayes argv and dependency admission for one local validation package.
//! This module does not launch a worker or certify runtime containment.

use super::{contained, err, fs, PathBuf, Payload, Result};
use crate::maintenance_source::DirectoryPin;
use std::{
    ffi::{CString, OsString},
    fs::File,
    mem::MaybeUninit,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::Path,
};

const SOURCE: &str = "77e8e4dc1c8489f42c3f1dd0a6d858eecc96bf1c";
const PRODUCER: &str = "153558a719595549071e9bf6dd06950a0bfba9549c463601c1c6ac0ff51cc3ba";
const MANIFEST: &str = "94a6551d16af3fb134f94549d9f087ee8ccb9fd2372d914de5f738a78d3505c6";
const NODE: &str = "bin/node";
const NODE_SHA: &str = "e4b5a3af0e05c75de2eae013904145f40fe7fc2a6e6f17510128bf45cca4e79b";
const NODE_NOTICE: &str = "notices/Node-LICENSE";
const NODE_NOTICE_SHA: &str = "5888dbb9a1d2b18f2c3e6c5f6af1b39de658372b402a0577b002777f14c62ace";
const WORKER: &str = "platform/apps/api/dist/hayes-original-worker.js";
const WORKER_SHA: &str = "c20f1a2f8720eec316695e1021c281141d4219252a0c28489167b68ed63e5cdf";
const VALIDATOR: &str = "platform/apps/api/dist/validation-worker.js";
const VALIDATOR_SHA: &str = "ffa24f35ad2e3358651f10610cbea372fcf709cfb6636128ea5fb7d9b670ff2c";
const VIPS: &str =
    "platform/node_modules/@img/sharp-libvips-darwin-arm64/lib/libvips-cpp.8.18.6.dylib";
const VIPS_SHA: &str = "bba7a98f1c4b10f4c31f92e12264e5d41a7ab484b864feae535ac8919c548d01";
const LOCKFILE_SHA: &str = "3f402eddf68ca77d5ce3ee72e4ee1ade4ffbdbf954f5f10e0414e5ead98408be";

const GENERATE_MODULES: &[&str] = &[
    "meshoptimizer",
    "sharp",
    "@img/colour",
    "detect-libc",
    "semver",
    "@img/sharp-darwin-arm64",
    "@img/sharp-libvips-darwin-arm64",
];
const VALIDATE_MODULES: &[&str] = &["gltf-validator", "@exhibitos/spec"];
const VALIDATE_FILES: &[&str] = &[
    VALIDATOR,
    "platform/apps/api/dist/approved-lod-pair.js",
    "platform/apps/api/dist/basis-payload.js",
    "platform/apps/api/dist/png-pixels.js",
    "platform/apps/api/dist/viewer-variants.js",
    "platform/apps/api/dist/watermark-label.js",
];

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum HayesPhase {
    Generate,
    Validate,
}

pub(crate) struct HayesCommand {
    pub(crate) program: PathBuf,
    /// Arguments before the native launcher appends lock identity and nonce.
    pub(crate) args: Vec<OsString>,
}

/// Owns the mounted directory identities across spawn and READY. Only the
/// exact, local Generate command may carry its one fixed environment key.
pub(crate) struct ReadOnlyHayesGenerate {
    root: DirectoryPin,
    notices: DirectoryPin,
    libdir: PathBuf,
    device: u64,
    command: HayesCommand,
    environment: CString,
}

impl ReadOnlyHayesGenerate {
    pub(crate) fn manifest_sha256(&self) -> &'static str {
        MANIFEST
    }

    pub(crate) fn check(&self) -> Result<()> {
        let root = self.root.try_clone_file().map_err(|_| unavailable())?;
        let notices = self.notices.try_clone_file().map_err(|_| unavailable())?;
        if readonly_device(&root)? != self.device || readonly_device(&notices)? != self.device {
            return Err(unavailable());
        }
        match fs::symlink_metadata(&self.libdir) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(unavailable()),
        }
    }

    pub(crate) fn command(&self) -> &HayesCommand {
        &self.command
    }

    pub(crate) fn environment(&self) -> &std::ffi::CStr {
        &self.environment
    }

    pub(crate) fn validate_command(&self, payload: &Payload) -> Result<HayesCommand> {
        self.root
            .check_path(&payload.root)
            .map_err(|_| unavailable())?;
        self.check()?;
        payload.hayes_fixed_command(HayesPhase::Validate)
    }
}

fn readonly_device(file: &File) -> Result<u64> {
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(unavailable());
    }
    if unsafe { stat.assume_init() }.f_flags & libc::MNT_RDONLY as u32 == 0 {
        return Err(unavailable());
    }
    Ok(file.metadata().map_err(|_| unavailable())?.dev())
}

impl Payload {
    /// This is a local-validation package admission only. The calling native
    /// owner still needs separately qualified operation and lifetime proof.
    pub(crate) fn hayes_fixed_command(&self, phase: HayesPhase) -> Result<HayesCommand> {
        if self.manifest.mode != "local-validation"
            || self.manifest.source_commit != SOURCE
            || self.manifest.producer_sha256 != PRODUCER
            || self.manifest_sha256 != MANIFEST
        {
            return Err(unavailable());
        }
        self.recheck()?;

        let program = self.hayes_file(NODE, true, Some(NODE_SHA))?;
        let worker = self.hayes_file(WORKER, false, Some(WORKER_SHA))?;
        self.hayes_file(NODE_NOTICE, false, Some(NODE_NOTICE_SHA))?;
        self.hayes_file("platform/package-lock.json", false, Some(LOCKFILE_SHA))?;
        self.hayes_file(VALIDATOR, false, Some(VALIDATOR_SHA))?;
        let mut allowed = vec![
            worker.clone(),
            self.hayes_file("platform/package.json", false, None)?,
            self.hayes_file("platform/apps/api/package.json", false, None)?,
        ];
        let (name, modules) = match phase {
            HayesPhase::Generate => ("generate", GENERATE_MODULES),
            HayesPhase::Validate => ("validate", VALIDATE_MODULES),
        };
        for module in modules {
            allowed.push(self.hayes_directory(&format!("platform/node_modules/{module}"))?);
        }
        if phase == HayesPhase::Validate {
            for relative in VALIDATE_FILES {
                allowed.push(self.hayes_file(relative, false, None)?);
            }
            let link = "platform/node_modules/@exhibitos/studio-contract";
            let target = self.hayes_directory("platform/packages/studio-contract")?;
            if !self
                .manifest
                .files
                .iter()
                .any(|item| item.path == link && item.link.is_some())
                || contained(&self.root, link)? != target
            {
                return Err(unavailable());
            }
            allowed.push(self.root.join(link));
            allowed.push(self.hayes_file(
                "platform/packages/studio-contract/package.json",
                false,
                None,
            )?);
            allowed.push(self.hayes_directory("platform/packages/studio-contract/dist")?);
        }

        let mut args = vec![
            OsString::from("--max-old-space-size=128"),
            OsString::from("--permission"),
        ];
        if phase == HayesPhase::Generate {
            args.push("--allow-addons".into());
        }
        for path in allowed {
            args.push(read_flag(&path)?);
        }
        args.extend([
            worker.into_os_string(),
            "model/gltf-binary".into(),
            name.into(),
        ]);
        Ok(HayesCommand { program, args })
    }

    pub(crate) fn hayes_read_only_generate(&self) -> Result<ReadOnlyHayesGenerate> {
        let root = DirectoryPin::open(&self.root).map_err(|_| unavailable())?;
        let device = readonly_device(&root.try_clone_file().map_err(|_| unavailable())?)?;
        let command = self.hayes_fixed_command(HayesPhase::Generate)?;
        self.hayes_file(VIPS, false, Some(VIPS_SHA))?;
        // A separate mount under the package is outside this guard, even if
        // that path happened to contain matching bytes during the recheck.
        for item in &self.manifest.files {
            if fs::metadata(self.root.join(&item.path))
                .map_err(|_| unavailable())?
                .dev()
                != device
            {
                return Err(unavailable());
            }
        }
        let notices_path = self.hayes_directory("notices")?;
        let notices = DirectoryPin::open(&notices_path).map_err(|_| unavailable())?;
        let mut env = b"VIPSHOME=".to_vec();
        env.extend_from_slice(notices_path.as_os_str().as_encoded_bytes());
        let environment = CString::new(env).map_err(|_| unavailable())?;
        let guard = ReadOnlyHayesGenerate {
            root,
            notices,
            libdir: notices_path.join("lib"),
            device,
            command,
            environment,
        };
        guard.check()?;
        Ok(guard)
    }

    fn hayes_file(&self, relative: &str, executable: bool, sha: Option<&str>) -> Result<PathBuf> {
        let path = self.root.join(relative);
        if !self.manifest.files.iter().any(|item| {
            item.path == relative
                && item.link.is_none()
                && item.executable == Some(executable)
                && item.bytes.is_some_and(|bytes| bytes > 0)
                && sha.is_none_or(|sha| item.sha256.as_deref() == Some(sha))
        }) || contained(&self.root, relative)? != path
        {
            return Err(unavailable());
        }
        Ok(path)
    }

    fn hayes_directory(&self, relative: &str) -> Result<PathBuf> {
        let path = self.root.join(relative);
        let prefix = format!("{relative}/");
        if contained(&self.root, relative)? != path
            || !fs::metadata(&path).map_err(|_| unavailable())?.is_dir()
            || !self
                .manifest
                .files
                .iter()
                .any(|item| item.path.starts_with(&prefix) && item.link.is_none())
        {
            return Err(unavailable());
        }
        Ok(path)
    }
}

fn read_flag(path: &Path) -> Result<OsString> {
    let text = path.to_str().ok_or_else(unavailable)?;
    if !path.is_absolute() || text.contains('*') {
        return Err(unavailable());
    }
    Ok(format!("--allow-fs-read={text}").into())
}

fn unavailable() -> crate::LifecycleError {
    err("HAYES_FIXED_PAYLOAD_UNAVAILABLE")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_permission_rejects_relative_and_wildcard() {
        assert!(read_flag(Path::new("relative")).is_err());
        assert!(read_flag(Path::new("/tmp/has*wildcard")).is_err());
        assert_eq!(
            read_flag(Path::new("/tmp/fixed path")).unwrap(),
            "--allow-fs-read=/tmp/fixed path"
        );
    }

    #[test]
    #[ignore]
    fn generate_environment_requires_the_exact_read_only_candidate() {
        let mount =
            PathBuf::from(std::env::var_os("HAYES_FIXED_PACKAGE").expect("mounted candidate"));
        let writable =
            PathBuf::from(std::env::var_os("HAYES_WRITABLE_PACKAGE").expect("writable original"));
        let payload = Payload::open(&mount).unwrap();
        let pinned = payload.hayes_read_only_generate().unwrap();
        pinned.check().unwrap();
        assert_eq!(
            pinned.environment().to_bytes(),
            format!("VIPSHOME={}", mount.join("notices").display()).as_bytes(),
        );
        assert_eq!(pinned.command.program, mount.join(NODE));
        assert_eq!(pinned.command.args.last().unwrap(), "generate");
        assert!(Payload::open(&writable)
            .unwrap()
            .hayes_read_only_generate()
            .is_err());
    }

    /// Exercise the actual package through the existing complete Payload
    /// inventory verifier. This test prepares argv and never starts Node.
    #[test]
    #[ignore]
    fn exact_candidate_preflight() {
        let root =
            PathBuf::from(std::env::var_os("HAYES_FIXED_PACKAGE").expect("private candidate path"));
        let payload = Payload::open(&root).unwrap();
        for phase in [HayesPhase::Generate, HayesPhase::Validate] {
            let command = payload.hayes_fixed_command(phase).unwrap();
            assert_eq!(command.program, root.join(NODE));
            let args = command.args;
            assert_eq!(args[0], "--max-old-space-size=128");
            assert_eq!(args[1], "--permission");
            assert_eq!(
                args.iter().any(|arg| arg == "--allow-addons"),
                phase == HayesPhase::Generate
            );
            assert_eq!(args[args.len() - 3], root.join(WORKER).as_os_str());
            assert_eq!(args[args.len() - 2], "model/gltf-binary");
            assert_eq!(
                args[args.len() - 1],
                if phase == HayesPhase::Generate {
                    "generate"
                } else {
                    "validate"
                }
            );
            assert!(!args
                .iter()
                .any(|arg| arg == "--allow-child-process" || arg == "--allow-worker"));
        }
    }
}
