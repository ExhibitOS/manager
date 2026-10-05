// SPDX-License-Identifier: Apache-2.0
//! Cumulative known host/export bytes, not a disk reservation or Engine-volume budget.
use super::*;
const GIB: u64 = 1024 * 1024 * 1024;
const SOURCE_FLOOR: u64 = 6 * GIB;
// Host manifest <=8MiB, bounded streaming framing <=4MiB, and existing256MiB margin.
const HOST_OVERHEAD: u64 = (8 + 4 + 256) * 1024 * 1024;
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCheckpointSpaceReceipt {
    pub host_profile_bytes: u64,
    pub image_export_bytes: u64,
    pub trust_archive_budget: u64,
    pub source_required_bytes: u64,
    pub destination_required_bytes: u64,
    pub source_available_bytes: u64,
    pub destination_available_bytes: u64,
    pub shared_filesystem: bool,
    pub engine_volume_space_reserved: bool,
    pub disk_space_reserved: bool,
}
fn budgets(host: u64, images: u64, trust: u64, shared: bool) -> crate::Result<(u64, u64)> {
    if images == 0 || images > 2 * GIB {
        return Err(crate::err("STORAGE_QUOTA"));
    }
    if host
        .checked_add(images)
        .is_none_or(|n| n > 64 * GIB - 8 * 1024 * 1024)
    {
        return Err(crate::err("HOST_CHECKPOINT_QUOTA"));
    }
    // First image export remains in the profile and is copied into host.bin.
    // Second export is retained outside the profile alongside host.bin/trust.bin.
    let source = images.checked_add(SOURCE_FLOOR);
    let destination = images
        .checked_mul(2)
        .and_then(|n| n.checked_add(host))
        .and_then(|n| n.checked_add(trust))
        .and_then(|n| n.checked_add(HOST_OVERHEAD));
    let (source, destination) = source
        .zip(destination)
        .ok_or_else(|| crate::err("STORAGE_QUOTA"))?;
    if shared {
        let total = source
            .checked_add(destination)
            .ok_or_else(|| crate::err("STORAGE_QUOTA"))?;
        Ok((total, total))
    } else {
        Ok((source, destination))
    }
}
fn enough(required: (u64, u64), available: (u64, u64)) -> crate::Result<()> {
    if available.0 < required.0 || available.1 < required.1 {
        return Err(crate::err("CHECKPOINT_STORAGE_INSUFFICIENT"));
    }
    Ok(())
}
fn same_filesystem(source: &Path, destination: &Path) -> crate::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(fs::symlink_metadata(source)
            .map_err(|_| crate::err("STORAGE_UNAVAILABLE"))?
            .dev()
            == fs::symlink_metadata(destination)
                .map_err(|_| crate::err("STORAGE_UNAVAILABLE"))?
                .dev())
    }
    #[cfg(not(unix))]
    {
        let _ = (source, destination);
        Err(crate::err("HOST_PLATFORM_UNVERIFIED"))
    }
}
impl ExecutionSession<'_> {
    /// Caller must hold all registered operation locks through checkpoint_host_borrowed.
    pub(super) fn checkpoint_storage(
        &self,
        destination: &Path,
    ) -> crate::Result<SourceCheckpointSpaceReceipt> {
        self.check()?;
        let plan = self
            .store
            .intent()
            .ok_or_else(|| crate::err("UPDATE_INTENT_MISSING"))?
            .update
            .plan();
        let (registry, _) = installations::load(&self.store.profile)?
            .ok_or_else(|| crate::err("UPDATE_SOURCE_CHANGED"))?;
        let entry = registry
            .installations
            .iter()
            .find(|e| e.id == plan.target_instance && e.kind == "recovery")
            .ok_or_else(|| crate::err("UPDATE_TARGET_UNREGISTERED"))?;
        let root = installations::root(&self.store.profile, entry);
        installations::private_directory(&root)?;
        let target = LifecycleService::open_retry_diagnostics(root.clone())?;
        let job = target
            .restoration_status()?
            .filter(|j| j.state == "completed")
            .ok_or_else(|| crate::err("UPDATE_SOURCE_PROOF_MISSING"))?;
        let work = root.join(format!("restore-{}", job.id));
        let receipt: crate::restoration::RestorationReceipt =
            crate::read_json(&crate::checked_path(&work, "receipt.json")?)?;
        source_deployment::authenticated_files(&self.source.root, &work, &receipt, plan)?;
        let raw = crate::installation_backup::source_bytes(
            &work.join("authenticated"),
            "manifest.json",
            16 * 1024 * 1024,
            true,
        )?;
        crate::restoration::RestorationBinding::from_plan(plan)?.authenticated(
            &receipt.backup_id,
            &receipt.authenticated_manifest_sha256,
            &raw,
        )?;
        let manifest = source_full_configuration::scope(&raw)?;
        let bytes = crate::installation_backup::source_bytes(
            &work.join("configuration"),
            "manager-image-inventory.json",
            1048576,
            true,
        )?;
        let images = source_images::bound_inventory(&bytes, &manifest, &self.source.manifest()?)?;
        let image_export_bytes = images
            .iter()
            .try_fold(0u64, |n, image| n.checked_add(image.bytes))
            .ok_or_else(|| crate::err("STORAGE_QUOTA"))?;
        let host_profile_bytes =
            profile_backup::checkpoint_host_size(&self.store.profile, &self._session)?;
        let trust_archive_budget = self
            .store
            .trust_archive_budget()
            .map_err(|e| crate::err(e.code()))?;
        let shared_filesystem = same_filesystem(&self.source.root, destination)?;
        let required = budgets(
            host_profile_bytes,
            image_export_bytes,
            trust_archive_budget,
            shared_filesystem,
        )?;
        let available = (
            fs2::available_space(&self.source.root)
                .map_err(|_| crate::err("STORAGE_UNAVAILABLE"))?,
            fs2::available_space(destination).map_err(|_| crate::err("STORAGE_UNAVAILABLE"))?,
        );
        enough(required, available)?;
        self.check()?;
        Ok(SourceCheckpointSpaceReceipt {
            host_profile_bytes,
            image_export_bytes,
            trust_archive_budget,
            source_required_bytes: required.0,
            destination_required_bytes: required.1,
            source_available_bytes: available.0,
            destination_available_bytes: available.1,
            shared_filesystem,
            engine_volume_space_reserved: false,
            disk_space_reserved: false,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_storage_counts_all_retained_exports_and_host_archive_once() {
        let host = 5 * GIB;
        let images = GIB;
        let trust = 65536;
        let (source, external) = budgets(host, images, trust, false).unwrap();
        assert_eq!(source, 7 * GIB);
        assert_eq!(external, 7 * GIB + trust + HOST_OVERHEAD);
        let shared = budgets(host, images, trust, true).unwrap();
        assert_eq!(shared, (source + external, source + external));
        assert!(enough(shared, (9 * GIB, 9 * GIB)).is_err());
        assert!(enough(shared, shared).is_ok());
        assert!(enough((source, external), (source, external - 1)).is_err());
        assert!(enough((source, external), (source - 1, external)).is_err());
    }
    #[test]
    fn impossible_or_overflowing_budget_refuses_before_any_write() {
        for images in [0, 2 * GIB + 1, u64::MAX] {
            assert!(budgets(0, images, 0, true).is_err());
        }
        assert!(budgets(u64::MAX, 1, 0, false).is_err());
        assert!(budgets(64 * GIB - 8 * 1024 * 1024, 1, 0, true).is_err());
        assert!(budgets(0, 1, u64::MAX, true).is_err());
    }
}
