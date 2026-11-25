//! A shell in a pseudo-terminal: the process, what is sent to it and how
//! big it is. What it prints is read by whoever opened it, whose screen it
//! is (the desk's `terminal.rs`).

use std::{io::Write as _, path::Path};

use portable_pty::{CommandBuilder, MasterPty, PtySize};

/// A shell running in a pseudo-terminal. It is stopped when dropped.
pub struct Pty {
    /// What is sent to it, written by a thread of its own: a program that
    /// does not read would otherwise hold the sender as the pty fills.
    writer: std::sync::mpsc::Sender<Vec<u8>>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl Pty {
    /// Open your login shell in `dir`, or over SSH on `host` in `dir`
    /// there, `rows` by `cols`; `first`, if given, is typed into it once it
    /// starts (a script's command). Blocking: it starts a process. Handed
    /// back with it, what it is called in a tab (the shell, or `first`) and
    /// what reads what it prints.
    pub fn open(
        dir: &Path,
        host: Option<&str>,
        first: Option<&str>,
        (rows, cols): (u16, u16),
    ) -> Result<(Self, String, Box<dyn std::io::Read + Send>), String> {
        let pty = portable_pty::native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("no terminal could be opened: {e}"))?;
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let (mut command, title) = if let Some(host) = host {
            let mut c = CommandBuilder::new("ssh");
            let there = crate::remote::shell_in(dir, "exec \"$SHELL\" -l");
            c.args(["-t", host, &there]);
            if let Some(home) = std::env::var_os("HOME") {
                c.cwd(home);
            }
            (c, format!("ssh {host}"))
        } else {
            let mut c = CommandBuilder::new(&shell);
            c.arg("-l");
            c.cwd(dir);
            let name = Path::new(&shell)
                .file_name()
                .map_or_else(|| shell.clone(), |n| n.to_string_lossy().into_owned());
            (c, name)
        };
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        // macOS's bash would otherwise open every terminal with a notice
        // that zsh is the default now.
        command.env("BASH_SILENCE_DEPRECATION_WARNING", "1");
        let child = pty
            .slave
            .spawn_command(command)
            .map_err(|e| format!("{title} could not be started: {e}"))?;
        let reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
        let mut writer = pty.master.take_writer().map_err(|e| e.to_string())?;
        let (to_writer, sent) = std::sync::mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            for bytes in sent {
                if writer
                    .write_all(&bytes)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        let title = match first {
            Some(line) => {
                let _ = to_writer.send(format!("{line}\r").into_bytes());
                line.to_owned()
            }
            None => title,
        };
        let pty = Self {
            writer: to_writer,
            master: pty.master,
            child,
        };
        Ok((pty, title, reader))
    }

    /// Send `bytes`, without waiting for them to be read.
    pub fn send(&self, bytes: &[u8]) {
        let _ = self.writer.send(bytes.to_vec());
    }

    /// Make it `rows` by `cols`.
    pub fn resize(&self, rows: u16, cols: u16) {
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    /// Whether something runs in it besides its shell: a command not yet
    /// done holds the terminal's foreground, which the shell holds
    /// otherwise.
    pub fn busy(&self) -> bool {
        let shell = self.child.process_id();
        (self.master.process_group_leader()).is_some_and(|front| u32::try_from(front).ok() != shell)
    }

    /// Stop what runs in it.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }

    /// How it ended.
    pub fn ended(&mut self) -> String {
        match self.child.try_wait() {
            Ok(Some(status)) if status.success() => "exited".to_owned(),
            Ok(Some(status)) => format!("exit {}", status.exit_code()),
            _ => "ended".to_owned(),
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_sent_to_a_program_that_does_not_read_does_not_wait() {
        let (pty, _, mut from) = Pty::open(
            &std::env::temp_dir(),
            None,
            // Raw, so that its input fills rather than being dropped.
            Some("stty raw -echo; sleep 5"),
            (6, 40),
        )
        .expect("a terminal opens");
        // What it prints is read, as the desk reads it, so that the shell
        // gets as far as the command.
        std::thread::spawn(move || std::io::copy(&mut from, &mut std::io::sink()));
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(pty.busy(), "sleep holds the terminal");
        let began = std::time::Instant::now();
        pty.send(&vec![b'x'; 1 << 20]);
        assert!(began.elapsed() < std::time::Duration::from_secs(1));
    }
}
