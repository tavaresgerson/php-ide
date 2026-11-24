//! A pseudo-terminal running a program (the user's shell).
//!
//! `openpty` gives a master/slave pair; the child gets the slave as its
//! controlling terminal and stdio, in a session of its own. A thread reads
//! the master into a channel so the UI never blocks; writes go straight to
//! the master. Resizing sets the window size, and the kernel tells the
//! foreground process with `SIGWINCH`.

use std::ffi::CString;
use std::io;
use std::path::Path;
use std::sync::mpsc;

/// What to start in the terminal.
#[derive(Debug, Clone)]
pub struct Command {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: std::path::PathBuf,
    /// Added to (or replacing in) the inherited environment.
    pub env: Vec<(String, String)>,
}

impl Command {
    /// The user's login shell (`$SHELL`, else `/bin/bash`, else `/bin/sh`),
    /// interactive, in `cwd`.
    pub fn shell(cwd: &Path) -> Command {
        let program = std::env::var("SHELL")
            .ok()
            .filter(|s| Path::new(s).exists())
            .or_else(|| {
                Path::new("/bin/bash")
                    .exists()
                    .then(|| "/bin/bash".to_string())
            })
            .unwrap_or_else(|| "/bin/sh".to_string());
        Command {
            args: Vec::new(),
            program,
            cwd: cwd.to_path_buf(),
            env: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// Output from the terminal's reader thread.
#[derive(Debug)]
pub enum PtyEvent {
    Output(Vec<u8>),
    /// The program exited (the master hit end of file).
    Exited,
}

/// A running program on a pseudo-terminal.
pub struct Pty {
    master: i32,
    pid: i32,
    events: mpsc::Receiver<PtyEvent>,
    exited: bool,
}

impl Pty {
    /// Starts `command` on a new pseudo-terminal of `cols`×`rows`.
    pub fn spawn(command: &Command, cols: u16, rows: u16) -> io::Result<Pty> {
        Self::spawn_with_waker(command, cols, rows, None)
    }

    /// Like [`Pty::spawn`]; `waker` runs on the reader thread after each
    /// chunk of output (the UI asks for a repaint there).
    #[cfg(unix)]
    pub fn spawn_with_waker(
        command: &Command,
        cols: u16,
        rows: u16,
        waker: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    ) -> io::Result<Pty> {
        // Everything the child needs is prepared before `fork`: after it,
        // only async-signal-safe calls are allowed.
        let program = CString::new(command.program.as_str())?;
        let mut argv: Vec<CString> = vec![CString::new(
            Path::new(&command.program)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| command.program.clone()),
        )?];
        for arg in &command.args {
            argv.push(CString::new(arg.as_str())?);
        }
        let mut argv_ptrs: Vec<*const libc::c_char> = argv.iter().map(|a| a.as_ptr()).collect();
        argv_ptrs.push(std::ptr::null());

        let mut env: Vec<(String, String)> = std::env::vars()
            .filter(|(k, _)| !command.env.iter().any(|(ck, _)| ck == k))
            .collect();
        env.extend(command.env.iter().cloned());
        let envp: Vec<CString> = env
            .iter()
            .filter_map(|(k, v)| CString::new(format!("{k}={v}")).ok())
            .collect();
        let mut envp_ptrs: Vec<*const libc::c_char> = envp.iter().map(|e| e.as_ptr()).collect();
        envp_ptrs.push(std::ptr::null());
        let cwd = CString::new(command.cwd.to_string_lossy().as_bytes())?;

        let mut master: libc::c_int = -1;
        let mut slave: libc::c_int = -1;
        let size = libc::winsize {
            ws_row: rows.max(1),
            ws_col: cols.max(1),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: plain FFI with valid out-pointers.
        let opened = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &size,
            )
        };
        if opened != 0 {
            return Err(io::Error::last_os_error());
        }
        // The master must not leak into other children of the IDE.
        unsafe {
            libc::fcntl(master, libc::F_SETFD, libc::FD_CLOEXEC);
        }

        // SAFETY: the child only calls async-signal-safe functions before
        // `execve` (or `_exit`).
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            let error = io::Error::last_os_error();
            unsafe {
                libc::close(master);
                libc::close(slave);
            }
            return Err(error);
        }
        if pid == 0 {
            unsafe {
                libc::setsid();
                libc::ioctl(slave, libc::TIOCSCTTY as _, 0);
                libc::dup2(slave, 0);
                libc::dup2(slave, 1);
                libc::dup2(slave, 2);
                if slave > 2 {
                    libc::close(slave);
                }
                libc::close(master);
                // Default signal dispositions for the shell.
                for signal in [
                    libc::SIGCHLD,
                    libc::SIGHUP,
                    libc::SIGINT,
                    libc::SIGQUIT,
                    libc::SIGTERM,
                    libc::SIGPIPE,
                ] {
                    libc::signal(signal, libc::SIG_DFL);
                }
                libc::chdir(cwd.as_ptr());
                libc::execve(program.as_ptr(), argv_ptrs.as_ptr(), envp_ptrs.as_ptr());
                // execve failed: try the PATH, then give up.
                libc::execvp(program.as_ptr(), argv_ptrs.as_ptr());
                libc::_exit(127);
            }
        }
        unsafe {
            libc::close(slave);
        }

        let (tx, events) = mpsc::channel();
        let reader = unsafe { libc::dup(master) };
        std::thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                let mut buf = vec![0u8; 16 * 1024];
                loop {
                    // SAFETY: reading into our own buffer.
                    let n = unsafe { libc::read(reader, buf.as_mut_ptr().cast(), buf.len()) };
                    if n > 0 {
                        if tx
                            .send(PtyEvent::Output(buf[..n as usize].to_vec()))
                            .is_err()
                        {
                            break;
                        }
                        if let Some(waker) = &waker {
                            waker();
                        }
                    } else if n < 0
                        && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted
                    {
                        continue;
                    } else {
                        // EOF or EIO: the slave side is closed.
                        let _ = tx.send(PtyEvent::Exited);
                        if let Some(waker) = &waker {
                            waker();
                        }
                        break;
                    }
                }
                unsafe {
                    libc::close(reader);
                }
            })?;

        Ok(Pty {
            master,
            pid,
            events,
            exited: false,
        })
    }

    #[cfg(not(unix))]
    pub fn spawn_with_waker(
        _command: &Command,
        _cols: u16,
        _rows: u16,
        _waker: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    ) -> io::Result<Pty> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "terminals need a Unix system",
        ))
    }

    /// Output read since the last call, and whether the program exited.
    pub fn read(&mut self) -> (Vec<u8>, bool) {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            match event {
                PtyEvent::Output(bytes) => out.extend(bytes),
                PtyEvent::Exited => self.exited = true,
            }
        }
        if self.exited {
            self.reap();
        }
        (out, self.exited)
    }

    /// Waits up to `timeout` for output (tests and short commands).
    pub fn read_timeout(&mut self, timeout: std::time::Duration) -> (Vec<u8>, bool) {
        let mut out = Vec::new();
        if let Ok(event) = self.events.recv_timeout(timeout) {
            match event {
                PtyEvent::Output(bytes) => out.extend(bytes),
                PtyEvent::Exited => self.exited = true,
            }
        }
        let (more, exited) = self.read();
        out.extend(more);
        (out, exited)
    }

    pub fn exited(&self) -> bool {
        self.exited
    }

    pub fn pid(&self) -> i32 {
        self.pid
    }

    /// Sends input (keys, paste) to the program.
    pub fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let mut rest = bytes;
        while !rest.is_empty() {
            // SAFETY: writing from a valid slice.
            let n = unsafe { libc::write(self.master, rest.as_ptr().cast(), rest.len()) };
            if n < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            rest = &rest[n as usize..];
        }
        Ok(())
    }

    /// New window size; the foreground program gets `SIGWINCH`.
    pub fn resize(&self, cols: u16, rows: u16) {
        let size = libc::winsize {
            ws_row: rows.max(1),
            ws_col: cols.max(1),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: valid fd and struct.
        unsafe {
            libc::ioctl(self.master, libc::TIOCSWINSZ, &size);
        }
    }

    /// The working directory of the foreground process (Linux `/proc`).
    pub fn cwd(&self) -> Option<std::path::PathBuf> {
        let group = unsafe { libc::tcgetpgrp(self.master) };
        let pid = if group > 0 { group } else { self.pid };
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }

    /// Ends the session: `SIGHUP` to the process group, like closing a
    /// terminal window.
    pub fn kill(&mut self) {
        if !self.exited {
            unsafe {
                libc::kill(-self.pid, libc::SIGHUP);
                libc::kill(self.pid, libc::SIGHUP);
            }
        }
        self.reap();
    }

    fn reap(&mut self) {
        let mut status = 0;
        // SAFETY: non-blocking wait on our own child.
        unsafe {
            libc::waitpid(self.pid, &mut status, libc::WNOHANG);
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.kill();
        unsafe {
            libc::close(self.master);
        }
    }
}
