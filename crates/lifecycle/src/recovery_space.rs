// SPDX-License-Identifier: Apache-2.0
//! Reserve exact authenticated bounded image exports plus unchanged recovery margins.
use super::*;
const GIB: u64 = 1024 * 1024 * 1024;
const EXPORT_CAP: u64 = 2 * GIB;
const HEADROOM: u64 = 2 * GIB;
const FLOOR: u64 = 6 * GIB;
fn required(bytes: impl IntoIterator<Item = u64>, observations: u64) -> crate::Result<u64> {
    if !(1..=3).contains(&observations) {
        return Err(crate::err("STORAGE_QUOTA"));
    }
    let sum = bytes.into_iter().try_fold(0u64, |sum, n| {
        if n == 0 {
            return Err(crate::err("STORAGE_QUOTA"));
        }
        sum.checked_add(n)
            .filter(|sum| *sum <= EXPORT_CAP)
            .ok_or_else(|| crate::err("STORAGE_QUOTA"))
    })?;
    if sum == 0 {
        return Err(crate::err("STORAGE_QUOTA"));
    }
    sum.checked_mul(observations)
        .and_then(|growth| growth.checked_add(HEADROOM + FLOOR))
        .ok_or_else(|| crate::err("STORAGE_QUOTA"))
}
fn with_host_reserve(export_budget: u64, host_bytes: u64) -> crate::Result<u64> {
    export_budget
        .checked_add(host_bytes)
        .ok_or_else(|| crate::err("STORAGE_QUOTA"))
}
pub(super) fn check(
    source: &LifecycleService,
    ctx: &candidate_inventory::CandidateContext<'_>,
    paths: &[&Path],
    observations: u64,
) -> crate::Result<u64> {
    check_with_host(source, ctx, paths, observations, 0)
}
pub(super) fn check_with_host(
    source: &LifecycleService,
    ctx: &candidate_inventory::CandidateContext<'_>,
    paths: &[&Path],
    observations: u64,
    host_bytes: u64,
) -> crate::Result<u64> {
    // CandidateContext's complete raw manifest is authenticated against the retained plan.
    // Do not accept caller sizes, image metadata estimates, or unauthenticated inventory.
    let manifest = source_full_configuration::scope(ctx.raw)?;
    let original = source.manifest()?;
    let bytes = crate::installation_backup::source_bytes(
        &ctx.workspace.join("configuration"),
        "manager-image-inventory.json",
        1048576,
        true,
    )?;
    let images = source_images::bound_inventory(&bytes, &manifest, &original)?;
    let needed = with_host_reserve(
        required(images.iter().map(|i| i.bytes), observations)?,
        host_bytes,
    )?;
    if paths.is_empty() {
        return Err(crate::err("STORAGE_UNAVAILABLE"));
    }
    for path in paths {
        let available =
            fs2::available_space(path).map_err(|_| crate::err("STORAGE_UNAVAILABLE"))?;
        if available < needed {
            return Err(crate::err("RESTORE_SPACE_REQUIRED"));
        }
    }
    Ok(needed)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_host_growth_never_consumes_export_headroom_or_floor() {
        let exports = required([100, 200], 3).unwrap();
        assert_eq!(with_host_reserve(exports, 5 * GIB).unwrap(), 13 * GIB + 900);
        assert_eq!(with_host_reserve(exports, 0).unwrap(), exports);
        assert_eq!(
            with_host_reserve(exports, u64::MAX).unwrap_err().code,
            "STORAGE_QUOTA"
        );
    }
    #[test]
    fn authenticated_growth_keeps_cap_headroom_floor_and_all_observations() {
        assert_eq!(required([100, 200], 1).unwrap(), 8 * GIB + 300);
        assert_eq!(required([100, 200], 3).unwrap(), 8 * GIB + 900);
        assert_eq!(required([GIB, GIB], 1).unwrap(), 10 * GIB);
        assert_eq!(required([GIB, GIB], 3).unwrap(), 14 * GIB);
        for sizes in [vec![], vec![0], vec![GIB, GIB, 1], vec![u64::MAX, 1]] {
            assert_eq!(required(sizes, 3).unwrap_err().code, "STORAGE_QUOTA");
        }
        for count in [0, 4, u64::MAX] {
            assert_eq!(required([1], count).unwrap_err().code, "STORAGE_QUOTA");
        }
    }
}
