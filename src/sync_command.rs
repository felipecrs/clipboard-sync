use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

/// Manages a sync command subprocess.
pub struct SyncCommand {
    child: Option<Child>,
}

impl SyncCommand {
    pub fn new() -> Self {
        Self { child: None }
    }

    /// Start the sync command if not already running.
    ///
    /// The command string is parsed using shell word splitting rules,
    /// properly handling quotes and escapes. The first token is the
    /// program to execute and the remaining tokens are its arguments.
    ///
    /// Returns `true` if a new process was spawned, `false` otherwise.
    pub fn start(&mut self, command: &str) -> bool {
        if command.is_empty() || self.child.is_some() {
            return false;
        }

        log::info!("Command: {command}");

        let parts = match shell_words::split(command) {
            Ok(parts) => parts,
            Err(e) => {
                log::error!("Failed to parse sync command: {e}");
                return false;
            }
        };

        let Some((program, args)) = parts.split_first() else {
            return false;
        };

        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        match cmd.spawn() {
            Ok(mut child) => {
                // Spawn thread to forward stdout to logs
                if let Some(stdout) = child.stdout.take() {
                    std::thread::spawn(move || {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines() {
                            if let Ok(line) = line {
                                log::info!("[sync-command] {line}");
                            }
                        }
                    });
                }

                // Spawn thread to forward stderr to logs
                if let Some(stderr) = child.stderr.take() {
                    std::thread::spawn(move || {
                        let reader = BufReader::new(stderr);
                        for line in reader.lines() {
                            if let Ok(line) = line {
                                log::warn!("[sync-command] {line}");
                            }
                        }
                    });
                }

                self.child = Some(child);
                true
            }
            Err(e) => {
                log::error!("Failed to start sync command: {e}");
                false
            }
        }
    }

    /// Stop the sync command if running.
    pub fn stop(&mut self) {
        if let Some(ref mut child) = self.child {
            log::info!("Stopping sync command...");
            let _ = child.kill();
            let _ = child.wait();
            log::info!("Sync command stopped.");
        }
        self.child = None;
    }

    /// Check if the sync command has exited. Returns the exit status if it did.
    pub fn check(&mut self) -> Option<std::process::ExitStatus> {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    log::warn!("Sync command exited with status: {status}");
                    self.child = None;
                    Some(status)
                }
                Ok(None) => None, // Still running
                Err(e) => {
                    log::error!("Error checking sync command status: {e}");
                    None
                }
            }
        } else {
            None
        }
    }
}

impl Drop for SyncCommand {
    fn drop(&mut self) {
        self.stop();
    }
}
