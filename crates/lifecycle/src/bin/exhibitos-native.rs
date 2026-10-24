// SPDX-License-Identifier: Apache-2.0
//! Private inherited stdio; fixed commands only. Account input is never echoed.
use exhibitos_lifecycle::installations::{InstallationController, native_ui::Request};
use serde_json::json;
use std::io::{self, Write};
#[cfg(not(target_os = "macos"))]
use std::io::{BufRead, Read};

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 || args[1] != "--profile" || !std::path::Path::new(&args[2]).is_absolute() {
        eprintln!("NATIVE_PROFILE_REQUIRED");
        std::process::exit(2);
    }
    let controller = match InstallationController::new((&args[2]).into(), None) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e.code);
            std::process::exit(1);
        }
    };
    std::process::exit(run(controller));
}

#[cfg(not(target_os = "macos"))]
fn run(controller: InstallationController) -> i32 {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    loop {
        let mut line = Vec::new();
        // Limit before allocation. Oversized/truncated frames terminate the session.
        match input.by_ref().take(3_145_729).read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) if line.len() <= 3_145_728 && line.last() == Some(&b'\n') => {}
            _ => {
                eprintln!("NATIVE_PROTOCOL_INVALID");
                std::process::exit(2);
            }
        }
        let response = match serde_json::from_slice::<Request>(&line) {
            Ok(request) => {
                let id = request.id;
                match request.execute(&controller) {
                    Ok(value) => json!({"version":1,"id":id,"result":value}),
                    Err(error) => json!({"version":1,"id":id,"error":{"code":error.code}}),
                }
            }
            Err(_) => json!({"version":1,"id":null,"error":{"code":"NATIVE_PROTOCOL_INVALID"}}),
        };
        if serde_json::to_writer(&mut output, &response).is_err()
            || output.write_all(b"\n").is_err()
            || output.flush().is_err()
        {
            break;
        }
    }
    0
}

#[cfg(target_os = "macos")]
static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(target_os = "macos")]
extern "C" fn shutdown_signal(_: libc::c_int) {
    STOP.store(true, std::sync::atomic::Ordering::SeqCst);
    exhibitos_lifecycle::native_runtime::request_shutdown();
}

#[cfg(target_os = "macos")]
fn run(controller: InstallationController) -> i32 {
    use exhibitos_lifecycle::native_host::Host;
    use std::sync::{Arc, atomic::Ordering};
    // Signal handlers only publish atomics. Child shutdown is owned by Host on
    // the normal thread, including EOF after the parent app disappears.
    unsafe {
        libc::signal(
            libc::SIGTERM,
            shutdown_signal as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGINT,
            shutdown_signal as *const () as libc::sighandler_t,
        );
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let Some(resources) = std::env::current_exe().ok().and_then(|p| {
        p.parent()?
            .parent()
            .map(|p| p.join("Resources/native-runtime"))
    }) else {
        eprintln!("NATIVE_HELPER_MISSING");
        return 1;
    };
    let mut host = Host::new(Arc::new(controller), resources);
    let mut output = io::stdout().lock();
    let mut exit_code = 0;
    let mut frame = Vec::with_capacity(3_145_728);
    let mut bytes = [0u8; 4096];
    'session: while !STOP.load(Ordering::SeqCst) {
        let mut fd = libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut fd, 1, 100) };
        if ready < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            break;
        }
        if ready == 0 {
            continue;
        }
        let count =
            unsafe { libc::read(libc::STDIN_FILENO, bytes.as_mut_ptr().cast(), bytes.len()) };
        if count < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if count <= 0 {
            if !frame.is_empty() {
                exit_code = 2;
            }
            break;
        }
        for byte in &bytes[..count as usize] {
            if frame.len() == 3_145_728 {
                eprintln!("NATIVE_PROTOCOL_INVALID");
                exit_code = 2;
                break 'session;
            }
            frame.push(*byte);
            if *byte != b'\n' {
                continue;
            }
            let request = serde_json::from_slice::<Request>(&frame);
            frame.fill(0);
            frame.clear();
            let response = match request {
                Ok(request) => {
                    let id = request.id;
                    match host.handle(request) {
                        Ok(value) => json!({"version":1,"id":id,"result":value}),
                        Err(error) => json!({"version":1,"id":id,"error":{"code":error.code}}),
                    }
                }
                Err(_) => json!({"version":1,"id":null,"error":{"code":"NATIVE_PROTOCOL_INVALID"}}),
            };
            if serde_json::to_writer(&mut output, &response).is_err()
                || output.write_all(b"\n").is_err()
                || output.flush().is_err()
            {
                break 'session;
            }
            if STOP.load(Ordering::SeqCst) {
                break 'session;
            }
        }
        bytes.fill(0);
    }
    frame.fill(0);
    bytes.fill(0);
    host.shutdown();
    exit_code
}
