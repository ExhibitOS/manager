// SPDX-License-Identifier: Apache-2.0
use exhibitos_lifecycle::{Action, LifecycleService};
use std::path::PathBuf;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if !matches!(args.len(), 4 | 6 | 7 | 9 | 12)
        || args[1] != "--root"
        || (args.len() == 6 && args[3] != "diagnose-retry")
        || (args.len() == 7
            && !matches!(
                args[3].as_str(),
                "verify-backup"
                    | "create-backup"
                    | "reconcile-helper"
                    | "cancel-maintenance"
                    | "reconcile-retry"
            ))
        || (args.len() == 9 && !matches!(args[3].as_str(), "restore-backup" | "retry-backup"))
        || (args.len() == 12 && args[3] != "retry-restoration")
    {
        eprintln!(
            "usage: exhibitos-manager --root <private absolute directory> detect|install|start|stop|restart|retry|status|jobs|logs|open-url|prepare-installation-backup|backup-jobs|restoration-status|helper-reconciliations|maintenance-context|maintenance-retries|retry-diagnostic-history; diagnose-retry <retry UUID> <destination root or --same-root>; reconcile-retry <retry UUID> <destination root or --same-root> --preserve-candidates; retry-backup <failed UUID> <trusted image ID> <external key> --preserve-candidates --external-writers-quiesced; retry-restoration <failed UUID> <new private root> <trusted image ID> <external key> <archive> <new port> --preserve-candidates --fresh-installation; cancel-maintenance <backup|restoration> <active UUID> --preserve-candidates; reconcile-helper <backup|restoration> <failed job UUID> --preserve-candidates; create-backup <trusted image ID> <private key file> --external-writers-quiesced; verify-backup <trusted image ID> <private key file> <archive directory>; restore-backup <trusted image ID> <private key file> <archive directory> <new loopback port> --fresh-installation"
        );
        std::process::exit(2);
    }
    let result = (|| -> Result<serde_json::Value, exhibitos_lifecycle::LifecycleError> {
        let s = if matches!(
            args[3].as_str(),
            "diagnose-retry" | "reconcile-retry" | "retry-diagnostic-history"
        ) {
            LifecycleService::open_retry_diagnostics(PathBuf::from(&args[2]))?
        } else {
            LifecycleService::new(PathBuf::from(&args[2]))?
        };
        let value = match args[3].as_str() {
            "retry-diagnostic-history" if args.len() == 4 => {
                serde_json::to_value(s.retry_diagnostic_history()?)
            }
            "diagnose-retry" if args.len() == 6 => {
                serde_json::to_value(s.diagnose_maintenance_retry(
                    &args[4],
                    if args[5] == "--same-root" {
                        None
                    } else {
                        Some(std::path::Path::new(&args[5]))
                    },
                )?)
            }
            "reconcile-retry" if args.len() == 7 => {
                serde_json::to_value(s.reconcile_maintenance_retry(
                    &args[4],
                    if args[5] == "--same-root" {
                        None
                    } else {
                        Some(std::path::Path::new(&args[5]))
                    },
                    args[6] == "--preserve-candidates",
                )?)
            }
            "maintenance-retries" if args.len() == 4 => {
                serde_json::to_value(s.maintenance_retries()?)
            }
            "retry-backup" if args.len() == 9 => serde_json::to_value(s.retry_backup(
                &args[4],
                &args[5],
                std::path::Path::new(&args[6]),
                args[7] == "--preserve-candidates",
                args[8] == "--external-writers-quiesced",
            )?),
            "retry-restoration" if args.len() == 12 => {
                if args[10] != "--preserve-candidates" || args[11] != "--fresh-installation" {
                    return Err(exhibitos_lifecycle::LifecycleError {
                        code: "RETRY_ACK_REQUIRED".into(),
                        guidance: "실패 후보 보존과 새 복원 공간 사용을 확인하세요.".into(),
                    });
                }
                let destination = LifecycleService::new(PathBuf::from(&args[5]))?;
                serde_json::to_value(s.retry_restoration(
                    &args[4],
                    exhibitos_lifecycle::retry::RestorationRetryInput {
                        destination: &destination,
                        image: &args[6],
                        key: std::path::Path::new(&args[7]),
                        archive: std::path::Path::new(&args[8]),
                        port: args[9].parse().unwrap_or(0),
                        preserve_candidates: true,
                        fresh_installation: true,
                    },
                )?)
            }
            "maintenance-context" => serde_json::to_value(s.maintenance_context()?),
            "cancel-maintenance" if args.len() == 7 => {
                serde_json::to_value(s.request_maintenance_cancel(
                    &args[4],
                    &args[5],
                    args[6] == "--preserve-candidates",
                )?)
            }
            "helper-reconciliations" => serde_json::to_value(s.helper_reconciliations()?),
            "reconcile-helper" if args.len() == 7 => serde_json::to_value(s.reconcile_helper(
                &args[4],
                &args[5],
                args[6] == "--preserve-candidates",
            )?),
            "restoration-status" => serde_json::to_value(s.restoration_status()?),
            "restore-backup" if args.len() == 9 => serde_json::to_value(s.restore_backup(
                &args[4],
                std::path::Path::new(&args[5]),
                std::path::Path::new(&args[6]),
                args[7].parse().unwrap_or(0),
                args[8] == "--fresh-installation",
            )?),
            "create-backup" if args.len() == 7 => serde_json::to_value(s.create_backup(
                &args[4],
                std::path::Path::new(&args[5]),
                args[6] == "--external-writers-quiesced",
            )?),
            "backup-jobs" => serde_json::to_value(s.backup_jobs()?),
            "verify-backup" if args.len() == 7 => serde_json::to_value(s.verify_backup(
                &args[4],
                std::path::Path::new(&args[5]),
                std::path::Path::new(&args[6]),
            )?),
            "prepare-installation-backup" => serde_json::to_value(s.prepare_installation_backup()?),
            "detect" => serde_json::to_value(s.detect()?),
            "install" => serde_json::to_value(s.install()?),
            "start" => serde_json::to_value(s.execute(Action::Start)?),
            "stop" => serde_json::to_value(s.execute(Action::Stop)?),
            "restart" => serde_json::to_value(s.execute(Action::Restart)?),
            "retry" => serde_json::to_value(s.execute(Action::Retry)?),
            "status" => serde_json::to_value(s.status()?),
            "jobs" => serde_json::to_value(s.jobs()?),
            "logs" => serde_json::to_value(s.logs()?),
            "open-url" => serde_json::to_value(s.open_url()?),
            _ => {
                return Err(exhibitos_lifecycle::LifecycleError {
                    code: "INVALID_ACTION".into(),
                    guidance: "지원되는 작업을 선택하세요.".into(),
                });
            }
        };
        Ok(value.unwrap_or(serde_json::Value::Null))
    })();
    match result {
        Ok(v) => {
            println!("{}", v);
            if v["state"] == "failed" && args[3] != "restoration-status" {
                std::process::exit(1);
            }
        }
        Err(e) => {
            println!("{}", serde_json::to_string(&e).unwrap());
            std::process::exit(1);
        }
    }
}
