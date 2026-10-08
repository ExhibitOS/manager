// SPDX-License-Identifier: Apache-2.0
//! Recovery may authenticate authority before recreating an absent host. Keep
//! its external pathname fence; never create or adopt a host to satisfy a lock.
use super::*;
use std::os::unix::fs::MetadataExt;

pub(crate) enum AuthorityHostFence {
    Present(Box<PresentHost>),
    Absent(Box<AbsentHost>),
}
pub(crate) struct PresentHost {
    session: ProfileSession,
    _locks: (File, Vec<File>),
}
pub(crate) struct AbsentHost {
    profile: PathBuf,
    parent_identity: fs::Metadata,
    anchor_identity: fs::Metadata,
    anchor_path: PathBuf,
    anchor: ProfileAnchor,
}
impl AuthorityHostFence {
    pub(crate) fn acquire(profile: &Path, anchor: ProfileAnchor) -> Result<Self> {
        match fs::symlink_metadata(profile) {
            Ok(_) => {
                let session = anchored_session(profile, anchor, true)?;
                let locks = current_host_locks(profile, &session)?;
                Ok(Self::Present(Box::new(PresentHost {
                    session,
                    _locks: locks,
                })))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let parent = canonical_private(
                    profile
                        .parent()
                        .ok_or_else(|| err("PROFILE_PATH_INVALID"))?,
                )?;
                let text = profile
                    .to_str()
                    .ok_or_else(|| err("PROFILE_PATH_INVALID"))?;
                let missing = AbsentHost {
                    profile: profile.into(),
                    parent_identity: fs::symlink_metadata(&parent)
                        .map_err(|_| err("PROFILE_PATH_INVALID"))?,
                    anchor_identity: anchor
                        .unix
                        .file
                        .metadata()
                        .map_err(|_| err("PROFILE_LOCK_INVALID"))?,
                    anchor_path: parent.join(format!(
                        ".exhibitos-profile-session-{}.lock",
                        digest(text.as_bytes())
                    )),
                    anchor,
                };
                missing.check()?;
                Ok(Self::Absent(Box::new(missing)))
            }
            Err(_) => Err(err("PROFILE_PATH_INVALID")),
        }
    }
    pub(crate) fn check(&self, profile: &Path) -> Result<()> {
        match self {
            Self::Present(held) => held.session.check_exclusive(profile),
            Self::Absent(held) if held.profile == profile => held.check(),
            Self::Absent(_) => Err(err("PROFILE_LOCK_INVALID")),
        }
    }
}
impl AbsentHost {
    fn check(&self) -> Result<()> {
        let parent = canonical_private(
            self.profile
                .parent()
                .ok_or_else(|| err("PROFILE_PATH_INVALID"))?,
        )?;
        let current = fs::symlink_metadata(parent).map_err(|_| err("PROFILE_PATH_INVALID"))?;
        if (current.dev(), current.ino())
            != (self.parent_identity.dev(), self.parent_identity.ino())
        {
            return Err(err("PROFILE_PATH_INVALID"));
        }
        match fs::symlink_metadata(&self.profile) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            _ => return Err(err("PROFILE_PATH_INVALID")),
        }
        let named =
            fs::symlink_metadata(&self.anchor_path).map_err(|_| err("PROFILE_LOCK_INVALID"))?;
        let held = self
            .anchor
            .unix
            .file
            .metadata()
            .map_err(|_| err("PROFILE_LOCK_INVALID"))?;
        for m in [&named, &held] {
            if !m.is_file()
                || m.is_symlink()
                || m.uid() != unsafe { libc::geteuid() }
                || m.mode() & 0o777 != 0o600
                || m.nlink() != 1
                || (m.dev(), m.ino()) != (self.anchor_identity.dev(), self.anchor_identity.ino())
            {
                return Err(err("PROFILE_LOCK_INVALID"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, PathBuf) {
        let parent = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "exhibitos-absent-host-fence-{}",
                uuid::Uuid::new_v4()
            ));
        installations::new_directory(&parent).unwrap();
        let profile = parent.join("profile");
        (parent, profile)
    }
    #[test]
    fn missing_host_fence_blocks_controllers_without_creating_or_adopting_host() {
        let (parent, profile) = fixture();
        let (_, anchor) = anchor_lock(&profile, true).unwrap();
        let fence = AuthorityHostFence::acquire(&profile, anchor).unwrap();
        fence.check(&profile).unwrap();
        assert!(!profile.exists());
        assert!(anchor_lock(&profile, false).is_err());
        installations::new_directory(&profile).unwrap();
        assert!(fence.check(&profile).is_err());
        assert!(profile.is_dir());
        drop(fence);
        fs::remove_dir_all(parent).unwrap();
    }
    #[test]
    fn missing_host_fence_refuses_replaced_parent_and_anchor_alias() {
        for parent_replacement in [true, false] {
            let (parent, profile) = fixture();
            let (_, anchor) = anchor_lock(&profile, true).unwrap();
            let fence = AuthorityHostFence::acquire(&profile, anchor).unwrap();
            if parent_replacement {
                let held = parent.with_extension("held");
                fs::rename(&parent, &held).unwrap();
                installations::new_directory(&parent).unwrap();
                assert!(fence.check(&profile).is_err());
                drop(fence);
                fs::remove_dir_all(held).unwrap();
            } else {
                let anchor_path = parent.join(format!(
                    ".exhibitos-profile-session-{}.lock",
                    digest(profile.to_str().unwrap().as_bytes())
                ));
                fs::hard_link(anchor_path, parent.join("untrusted-alias")).unwrap();
                assert!(fence.check(&profile).is_err());
                drop(fence);
            }
            fs::remove_dir_all(parent).unwrap();
        }
    }
}
