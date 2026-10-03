// SPDX-License-Identifier: Apache-2.0
use exhibitos_lifecycle::{Action, LifecycleService};
use std::path::PathBuf;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if !matches!(args.len(), 4 | 7)
        || args[1] != "--root"
        || (args.len() == 7 && args[3] != "verify-backup")
    {
        eprintln!(
            "usage: exhibitos-manager --root <private absolute directory> detect|install|start|stop|restart|retry|status|jobs|logs|open-url; verify-backup <trusted image ID> <private key file> <archive directory>"
        );
        std::process::exit(2);
    }
    let result = (|| -> Result<serde_json::Value, exhibitos_lifecycle::LifecycleError> {
        let s = LifecycleService::new(PathBuf::from(&args[2]))?;
        let value = match args[3].as_str() {
            "verify-backup" if args.len() == 7 => serde_json::to_value(s.verify_backup(
                &args[4],
                std::path::Path::new(&args[5]),
                std::path::Path::new(&args[6]),
            )?),
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
            if v["state"] == "failed" {
                std::process::exit(1);
            }
        }
        Err(e) => {
            println!("{}", serde_json::to_string(&e).unwrap());
            std::process::exit(1);
        }
    }
}
