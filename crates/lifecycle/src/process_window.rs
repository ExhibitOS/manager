// SPDX-License-Identifier: Apache-2.0
//! Noninteractive subprocesses must not create a Windows console during GUI polling.
use std::{ffi::OsStr, process::Command};

pub(crate) fn background_command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: keep piped I/O and exit status, without a console.
        command.creation_flags(0x0800_0000);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn background_process_preserves_output_and_failure_status() {
        let output = background_command("/bin/sh")
            .args([
                "-c",
                "printf synthetic-output; printf synthetic-error >&2; exit 7",
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout, b"synthetic-output");
        assert_eq!(output.stderr, b"synthetic-error");
    }
    #[cfg(windows)]
    #[test]
    fn windows_background_process_has_no_console_and_preserves_output() {
        let script = r#"Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class ConsoleProbe { [DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow(); }'; if ([ConsoleProbe]::GetConsoleWindow() -ne [IntPtr]::Zero) { exit 9 }; [Console]::Out.Write('synthetic-output'); [Console]::Error.Write('synthetic-error'); exit 7"#;
        let output = background_command("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                script,
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout, b"synthetic-output");
        assert_eq!(output.stderr, b"synthetic-error");
    }
}
