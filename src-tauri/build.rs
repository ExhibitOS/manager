// SPDX-License-Identifier: Apache-2.0
fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "manager_status",
            "manager_detect",
            "manager_install",
            "manager_action",
            "manager_jobs",
            "manager_logs",
            "manager_open_exhibition",
            "manager_verify_backup",
            "manager_create_backup",
            "manager_backup_jobs",
            "manager_restore_backup",
            "manager_restoration_context",
        ]),
    ))
    .expect("Manager desktop permission manifest must build");
}
