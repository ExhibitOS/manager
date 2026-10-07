// SPDX-License-Identifier: Apache-2.0
//! Offline profile metadata backup/recovery, separate from runtime lifecycle.
use exhibitos_lifecycle::profile_backup;
use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() >= 4 && a[1] == "--profile" {
        if a[3] == "checkpoint-host" && a.len() == 8 {
            emit(profile_backup::checkpoint_host(
                Path::new(&a[2]),
                Path::new(&a[4]),
                Path::new(&a[5]),
                a[6] == "--apps-closed",
                a[7] == "--host-writers-stopped",
            ));
            return;
        }
        if a[3] == "verify-host-current" && a.len() == 9 {
            emit(profile_backup::verify_host_current(
                Path::new(&a[2]),
                Path::new(&a[4]),
                Path::new(&a[5]),
                &a[6],
                a[7] == "--apps-closed",
                a[8] == "--host-writers-stopped",
            ));
            return;
        }
        if matches!(a[3].as_str(), "extract-host" | "extract-host-clone-only") && a.len() == 8 {
            let extract = if a[3] == "extract-host-clone-only" {
                profile_backup::extract_host_clone_only
            } else {
                profile_backup::extract_host
            };
            emit(extract(
                Path::new(&a[2]),
                Path::new(&a[4]),
                Path::new(&a[5]),
                Path::new(&a[6]),
                a[7] == "--extract-only",
            ));
            return;
        }
    }
    if a.len() != 7
        || a[1] != "--profile"
        || !["backup", "backup-stream", "restore"].contains(&a[3].as_str())
    {
        eprintln!(
            "usage: exhibitos-profile --profile <canonical private profile> backup|backup-stream|restore <external 32-byte private key> <external archive file> --apps-closed"
        );
        std::process::exit(2);
    }
    let result = if a[3] == "backup" {
        profile_backup::backup(
            Path::new(&a[2]),
            Path::new(&a[4]),
            Path::new(&a[5]),
            a[6] == "--apps-closed",
        )
    } else if a[3] == "backup-stream" {
        profile_backup::backup_stream(
            Path::new(&a[2]),
            Path::new(&a[4]),
            Path::new(&a[5]),
            a[6] == "--apps-closed",
        )
    } else {
        profile_backup::restore(
            Path::new(&a[2]),
            Path::new(&a[4]),
            Path::new(&a[5]),
            a[6] == "--apps-closed",
        )
    };
    emit(result);
}
fn emit<T: serde::Serialize>(result: std::result::Result<T, exhibitos_lifecycle::LifecycleError>) {
    match result {
        Ok(v) => println!("{}", serde_json::to_string(&v).unwrap()),
        Err(e) => {
            println!("{}", serde_json::to_string(&e).unwrap());
            std::process::exit(1);
        }
    }
}
