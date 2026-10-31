use std::fs::File;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, Signal, System};

/// Starts `exe` in the background, detached from the terminal, logging to `log`.
pub fn spawn_detached(exe: &Path, args: &[String], log: &Path, cwd: &Path) -> anyhow::Result<u32> {
    std::fs::create_dir_all(log.parent().unwrap())?;
    let out = File::options().create(true).append(true).open(log)?;
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(out);
    if let Some(dir) = exe.parent() {
        cmd.env(super::engine::lib_env_var(), dir);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    Ok(cmd.spawn()?.id())
}

fn refresh(sys: &mut System, pid: u32) {
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );
}

pub fn start_time(pid: u32) -> Option<u64> {
    let mut sys = System::new();
    refresh(&mut sys, pid);
    sys.process(Pid::from_u32(pid)).map(|p| p.start_time())
}

/// True if `pid` is alive and is the same process we started (start time matches).
pub fn is_ours(pid: u32, started: Option<u64>) -> bool {
    match (start_time(pid), started) {
        (Some(now), Some(then)) => now.abs_diff(then) <= 2,
        (Some(_), None) => true,
        _ => false,
    }
}

/// Resident memory of a process in bytes.
pub fn memory(pid: u32) -> Option<u64> {
    let mut sys = System::new();
    refresh(&mut sys, pid);
    sys.process(Pid::from_u32(pid)).map(|p| p.memory())
}

/// Graceful stop: terminate, wait up to 10 seconds, then force kill.
pub fn stop(pid: u32) {
    let mut sys = System::new();
    refresh(&mut sys, pid);
    let Some(p) = sys.process(Pid::from_u32(pid)) else {
        return;
    };
    if p.kill_with(Signal::Term).is_none() {
        p.kill();
    }
    for _ in 0..20 {
        sleep(Duration::from_millis(500));
        refresh(&mut sys, pid);
        match sys.process(Pid::from_u32(pid)) {
            None => return,
            Some(p)
                if matches!(
                    p.status(),
                    sysinfo::ProcessStatus::Zombie | sysinfo::ProcessStatus::Dead
                ) =>
            {
                return
            }
            _ => {}
        }
    }
    if let Some(p) = sys.process(Pid::from_u32(pid)) {
        p.kill();
    }
}

pub fn port_free(host: &str, port: u16) -> bool {
    let bind = if host == "localhost" { "127.0.0.1" } else { host };
    std::net::TcpListener::bind((bind, port)).is_ok()
}

pub fn next_free_port(host: &str, from: u16) -> Option<u16> {
    (from..from.saturating_add(100)).find(|p| port_free(host, *p))
}
