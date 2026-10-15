// SPDX-License-Identifier: Apache-2.0
fn main() {
    let path="remote-runtime/generated/manifest.json";
    println!("cargo:rerun-if-changed={path}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref()!=Ok("windows") { if let Ok(text)=std::fs::read_to_string(path) { println!("cargo:rustc-env=EXHIBITOS_REMOTE_RUNTIME_MANIFEST={}",text.trim()); }}

    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "manager_status",
            "manager_remote",
            "manager_remote_available",
            "manager_remote_cancel",
            "manager_update_status",
            "manager_update_cancel",
            "manager_update_references",
            "manager_update_apply",
            "manager_detect",
            "manager_install",
            "manager_action",
            "manager_jobs",
            "manager_logs",
            "manager_open_exhibition",
            "manager_open_oex",
            "manager_verify_backup",
            "manager_create_backup",
            "manager_backup_jobs",
            "manager_restore_backup",
            "manager_restoration_context",
            "manager_reconcile_helper",
            "manager_helper_reconciliations",
            "manager_maintenance_context",
            "manager_cancel_maintenance",
            "manager_maintenance_retries",
            "manager_retry_backup",
            "manager_retry_restoration",
            "manager_installations",
            "manager_create_installation",
            "manager_select_installation",
        ]),
    ))
    .expect("Manager desktop permission manifest must build");
}
