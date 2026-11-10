// SPDX-License-Identifier: Apache-2.0
//! Private inherited launch transport for one local model-adoption owner.

use super::{
    Connection, LocalAuthority, Result, Setup,
    hayes_bootstrap::{DedicatedOwnerPin, DedicatedProtocol, handshake_dedicated},
};
use crate::{
    err, maintenance_source::DirectoryPin, native_account_home::effective_home,
    native_model_adoption_stage::private_directory,
};
use serde::Serialize;
use std::{
    ffi::CStr,
    fs::{self, File},
    io,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use uuid::Uuid;

pub(super) const SOCKET_ENV: &str = "EXHIBITOS_NATIVE_MODEL_ADOPTION_FD";
pub(super) const ROOT_ENV: &str = "EXHIBITOS_NATIVE_MODEL_ADOPTION_ROOT_FD";
const STAGING_ROOT: &CStr = c"ExhibitOS Model Adoption Staging";

fn unavailable() -> crate::LifecycleError {
    err("NATIVE_MODEL_ADOPTION_UNAVAILABLE")
}

pub(in super::super) struct AdoptionSetup {
    socket: Box<Setup>,
    pin: DedicatedOwnerPin,
    root: DirectoryPin,
    root_child: Option<File>,
    root_device: String,
    root_inode: String,
    helper: PathBuf,
}

pub(in super::super) struct AdoptionConnection {
    _channel: Connection,
    _pin: DedicatedOwnerPin,
    _root: DirectoryPin,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Pin<'a> {
    version: u8,
    runtime_generation: &'a str,
    secret: &'a str,
    native_owner_id: Uuid,
    runtime_manifest_sha256: &'a str,
    helper: &'a str,
    root_device: &'a str,
    root_inode: &'a str,
}

fn open_staging_root(workspace: &Path, payload: &Path) -> Result<DirectoryPin> {
    let support = effective_home()
        .ok_or_else(unavailable)?
        .join("Library/Application Support");
    let root = support.join(STAGING_ROOT.to_str().map_err(|_| unavailable())?);
    if [workspace, payload]
        .into_iter()
        .any(|path| root.starts_with(path) || path.starts_with(&root))
    {
        return Err(unavailable());
    }
    let parent = DirectoryPin::open(&support).map_err(|_| unavailable())?;
    let directory = parent.try_clone_file().map_err(|_| unavailable())?;
    let parent_stat = directory.metadata().map_err(|_| unavailable())?;
    if parent_stat.uid() != unsafe { libc::geteuid() } || parent_stat.mode() & 0o022 != 0 {
        return Err(unavailable());
    }
    if unsafe { libc::mkdirat(directory.as_raw_fd(), STAGING_ROOT.as_ptr(), 0o700) } == 0 {
        directory.sync_all().map_err(|_| unavailable())?;
    } else if io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists {
        return Err(unavailable());
    }
    parent.check().map_err(|_| unavailable())?;
    let root = DirectoryPin::open(&root).map_err(|_| unavailable())?;
    parent.check().map_err(|_| unavailable())?;
    Ok(root)
}

impl AdoptionSetup {
    pub(super) fn for_runtime(
        generation: &str,
        manifest: &str,
        workspace: &Path,
        payload: &Path,
    ) -> Result<Self> {
        let helper = fs::canonicalize(std::env::current_exe().map_err(|_| unavailable())?)
            .map_err(|_| unavailable())?;
        let stat = fs::symlink_metadata(&helper).map_err(|_| unavailable())?;
        if helper.file_name().and_then(|name| name.to_str()) != Some("exhibitos-native")
            || !stat.is_file()
            || stat.nlink() != 1
            || stat.mode() & 0o111 == 0
            || stat.mode() & 0o022 != 0
            || helper.starts_with(workspace)
            || helper.starts_with(payload)
        {
            return Err(unavailable());
        }
        Self::new(
            generation,
            manifest,
            open_staging_root(workspace, payload)?,
            helper,
        )
    }

    fn new(generation: &str, manifest: &str, root: DirectoryPin, helper: PathBuf) -> Result<Self> {
        let generation_id = Uuid::parse_str(generation).map_err(|_| unavailable())?;
        if generation_id.to_string() != generation || helper.to_str().is_none() {
            return Err(unavailable());
        }
        let file = root.try_clone_file().map_err(|_| unavailable())?;
        let stat = file.metadata().map_err(|_| unavailable())?;
        if !private_directory(&stat) {
            return Err(unavailable());
        }
        let root_fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
        if !(10..=1023).contains(&root_fd) {
            if root_fd >= 0 {
                unsafe { libc::close(root_fd) };
            }
            return Err(unavailable());
        }
        let root_child = Some(unsafe { File::from_raw_fd(root_fd) });
        Ok(Self {
            socket: Box::new(Setup::new()?),
            pin: DedicatedOwnerPin::new(generation_id, manifest).map_err(|_| unavailable())?,
            root,
            root_child,
            root_device: stat.dev().to_string(),
            root_inode: stat.ino().to_string(),
            helper,
        })
    }

    fn check_root(&self) -> Result<()> {
        let file = self.root.try_clone_file().map_err(|_| unavailable())?;
        let stat = file.metadata().map_err(|_| unavailable())?;
        if !private_directory(&stat)
            || stat.dev().to_string() != self.root_device
            || stat.ino().to_string() != self.root_inode
        {
            return Err(unavailable());
        }
        Ok(())
    }

    pub(super) fn child_fds(&self) -> Result<[i32; 2]> {
        self.check_root()?;
        Ok([
            self.socket
                .child
                .as_ref()
                .ok_or_else(unavailable)?
                .as_raw_fd(),
            self.root_child
                .as_ref()
                .ok_or_else(unavailable)?
                .as_raw_fd(),
        ])
    }

    pub(super) fn close_child(&mut self) {
        self.socket.close_child();
        self.root_child.take();
    }

    pub(super) fn bootstrap_pin<'a>(
        &'a self,
        generation: &'a str,
        ordinary_secret: &str,
        hayes: Option<&super::hayes_bootstrap::HayesSetup>,
    ) -> Result<Pin<'a>> {
        if self.pin.runtime_generation().to_string() != generation
            || ordinary_secret == self.socket.secret
            || hayes.is_some_and(|setup| setup.has_secret(&self.socket.secret))
        {
            return Err(unavailable());
        }
        self.check_root()?;
        Ok(Pin {
            version: 1,
            runtime_generation: generation,
            secret: &self.socket.secret,
            native_owner_id: self.pin.native_owner_id(),
            runtime_manifest_sha256: self.pin.host_manifest_sha256(),
            helper: self.helper.to_str().ok_or_else(unavailable)?,
            root_device: &self.root_device,
            root_inode: &self.root_inode,
        })
    }

    pub(in super::super) fn handshake(
        mut self,
        authority: &LocalAuthority,
        generation: &str,
        cancelled: &AtomicBool,
    ) -> Result<AdoptionConnection> {
        if self.root_child.is_some() {
            return Err(unavailable());
        }
        self.check_root()?;
        let channel = handshake_dedicated(
            &mut self.socket,
            &self.pin,
            authority,
            generation,
            cancelled,
            DedicatedProtocol::Adoption,
        )?;
        self.check_root()?;
        Ok(AdoptionConnection {
            _channel: channel,
            _pin: self.pin,
            _root: self.root,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_runtime::FurnishingStart;
    use crate::native_runtime::owner_channel::hayes_bootstrap::HayesSetup;
    use serde_json::Value;
    use std::{
        io::{Read, Write},
        os::unix::{
            fs::{DirBuilderExt, PermissionsExt},
            net::UnixStream,
        },
        process::Command,
        time::Duration,
    };

    const GENERATION: &str = "11111111-1111-4111-8111-111111111111";
    const MANIFEST: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    struct Root(PathBuf);

    impl Root {
        fn new() -> Self {
            let root = fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!("manager-adoption-bootstrap-{}", Uuid::new_v4()));
            fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
            Self(root)
        }

        fn setup(&self) -> AdoptionSetup {
            AdoptionSetup::new(
                GENERATION,
                MANIFEST,
                DirectoryPin::open(&self.0).unwrap(),
                "/synthetic/Contents/Helpers/exhibitos-native".into(),
            )
            .unwrap()
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn authority() -> LocalAuthority {
        LocalAuthority {
            format: 1,
            workspace_id: "22222222-2222-4222-8222-222222222222".into(),
            tenant_id: "33333333-3333-4333-8333-333333333333".into(),
            principal_id: "44444444-4444-4444-8444-444444444444".into(),
            owner_uid: unsafe { libc::geteuid() },
            session_token: "synthetic-session".into(),
            csrf_token: "synthetic-csrf".into(),
        }
    }

    #[test]
    fn closed_bootstrap_binds_distinct_secrets_and_root_identity() {
        let root = Root::new();
        let auth = authority();
        let mut ordinary = Setup::new().unwrap();
        let legacy = ordinary
            .workspace_furnishing_bootstrap(&auth, GENERATION, FurnishingStart::Reopen)
            .unwrap();
        assert!(
            !String::from_utf8(legacy)
                .unwrap()
                .contains("adoptionOwnerChannel")
        );

        let adoption = root.setup();
        assert!(
            adoption
                .bootstrap_pin(&auth.workspace_id, &ordinary.secret, None)
                .is_err()
        );
        assert!(
            adoption
                .bootstrap_pin(GENERATION, &adoption.socket.secret, None)
                .is_err()
        );

        let hayes =
            HayesSetup::new(DedicatedOwnerPin::new(GENERATION.parse().unwrap(), MANIFEST).unwrap())
                .unwrap();
        let mut collision = root.setup();
        let hayes_pin =
            serde_json::to_value(hayes.bootstrap_pin(GENERATION, &ordinary.secret).unwrap())
                .unwrap();
        collision.socket.secret = hayes_pin["secret"].as_str().unwrap().into();
        assert!(
            collision
                .bootstrap_pin(GENERATION, &ordinary.secret, Some(&hayes))
                .is_err()
        );

        ordinary.adoption = Some(adoption);
        let without_hayes = ordinary
            .workspace_furnishing_bootstrap(&auth, GENERATION, FurnishingStart::Initialize)
            .unwrap();
        let without_hayes: Value = serde_json::from_slice(&without_hayes).unwrap();
        assert_eq!(without_hayes.as_object().unwrap().len(), 3);
        assert!(without_hayes.get("hayesOwnerChannel").is_none());
        assert!(without_hayes.get("adoptionOwnerChannel").is_some());

        fs::set_permissions(&root.0, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            ordinary
                .workspace_furnishing_bootstrap(&auth, GENERATION, FurnishingStart::Reopen)
                .is_err()
        );
        fs::set_permissions(&root.0, fs::Permissions::from_mode(0o700)).unwrap();

        ordinary.hayes = Some(hayes);
        assert!(
            ordinary
                .workspace_furnishing_bootstrap(
                    &auth,
                    GENERATION,
                    FurnishingStart::SelectedImport040
                )
                .is_err()
        );
        let wire = ordinary
            .workspace_furnishing_bootstrap(&auth, GENERATION, FurnishingStart::Reopen)
            .unwrap();
        let wire = String::from_utf8(wire).unwrap();
        assert!(
            wire.find("\"ownerChannel\"").unwrap() < wire.find("\"hayesOwnerChannel\"").unwrap()
        );
        assert!(
            wire.find("\"hayesOwnerChannel\"").unwrap()
                < wire.find("\"adoptionOwnerChannel\"").unwrap()
        );
        let parsed: Value = serde_json::from_str(&wire).unwrap();
        let pin = &parsed["adoptionOwnerChannel"];
        let stat = fs::metadata(&root.0).unwrap();
        assert_eq!(pin.as_object().unwrap().len(), 8);
        assert_eq!(pin["version"], 1);
        assert_eq!(pin["runtimeGeneration"], GENERATION);
        assert_eq!(pin["runtimeManifestSha256"], MANIFEST);
        assert_eq!(
            pin["helper"],
            "/synthetic/Contents/Helpers/exhibitos-native"
        );
        assert_eq!(pin["rootDevice"], stat.dev().to_string());
        assert_eq!(pin["rootInode"], stat.ino().to_string());
        assert_ne!(pin["secret"], parsed["ownerChannel"]["secret"]);
        assert_ne!(pin["secret"], parsed["hayesOwnerChannel"]["secret"]);
        assert!(
            pin["nativeOwnerId"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()
                .is_ok()
        );

        let mut command = Command::new("/usr/bin/true");
        ordinary.configure(&mut command).unwrap();
        let [socket, directory] = ordinary.adoption.as_ref().unwrap().child_fds().unwrap();
        assert_ne!(socket, directory);
        let environment: std::collections::BTreeMap<_, _> = command.get_envs().collect();
        for (name, descriptor) in [(SOCKET_ENV, socket), (ROOT_ENV, directory)] {
            assert_eq!(
                environment[std::ffi::OsStr::new(name)],
                Some(std::ffi::OsStr::new(&descriptor.to_string()))
            );
        }
        assert!(command.status().unwrap().success());
        ordinary.close_child();
        assert!(ordinary.adoption.as_ref().unwrap().child_fds().is_err());
    }

    #[test]
    fn dedicated_handshake_requires_exact_local_owner_ack() {
        let root = Root::new();
        let auth = authority();
        for wrong in [false, true] {
            let mut setup = root.setup();
            let child: std::os::fd::OwnedFd = setup
                .socket
                .child
                .as_ref()
                .unwrap()
                .try_clone()
                .unwrap()
                .into();
            let mut peer = UnixStream::from(child);
            peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let owner = if wrong {
                Uuid::new_v4()
            } else {
                setup.pin.native_owner_id()
            };
            let ack = format!(
                "{{\"version\":1,\"kind\":\"adoption-owner-hello-ack\",\"workspaceId\":\"{}\",\"tenantId\":\"{}\",\"principalId\":\"{}\",\"runtimeGeneration\":\"{GENERATION}\",\"nativeOwnerId\":\"{owner}\",\"runtimeManifestSha256\":\"{MANIFEST}\"}}",
                auth.workspace_id, auth.tenant_id, auth.principal_id,
            );
            let hello = format!(
                "{{\"version\":1,\"kind\":\"adoption-owner-hello\",\"workspaceId\":\"{}\",\"tenantId\":\"{}\",\"principalId\":\"{}\",\"runtimeGeneration\":\"{GENERATION}\",\"secret\":\"{}\",\"nativeOwnerId\":\"{}\",\"runtimeManifestSha256\":\"{MANIFEST}\"}}",
                auth.workspace_id,
                auth.tenant_id,
                auth.principal_id,
                setup.socket.secret,
                setup.pin.native_owner_id(),
            );
            setup.close_child();
            let worker = std::thread::spawn(move || {
                let mut header = [0; 4];
                peer.read_exact(&mut header).unwrap();
                let mut body = vec![0; u32::from_be_bytes(header) as usize];
                peer.read_exact(&mut body).unwrap();
                assert_eq!(body, hello.as_bytes());
                peer.write_all(&(ack.len() as u32).to_be_bytes()).unwrap();
                peer.write_all(ack.as_bytes()).unwrap();
            });
            let result = setup.handshake(&auth, GENERATION, &AtomicBool::new(false));
            worker.join().unwrap();
            assert_eq!(result.is_ok(), !wrong);
        }
    }
}
