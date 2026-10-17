// SPDX-License-Identifier: Apache-2.0
use exhibitos_lifecycle::{LifecycleError, LifecycleService};
use std::path::Path;
pub fn prepare(resources: &Path, service: &LifecycleService) -> Result<(), LifecycleError> {
    service.prepare_bundled_runtime(&resources.join("platform-runtime/generated"))
}
