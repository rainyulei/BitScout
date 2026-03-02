use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Returns the ~/.bitscout directory path.
fn bitscout_dir() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join(".bitscout")
}

/// Check if a process with the given PID is alive using kill(pid, 0).
fn is_process_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Read the stored daemon PID from ~/.bitscout/daemon.pid.
fn read_pid() -> Option<i32> {
    let pid_path = bitscout_dir().join("daemon.pid");
    let contents = fs::read_to_string(pid_path).ok()?;
    contents.trim().parse::<i32>().ok()
}

pub fn execute(watch: &str) {
    // Check if daemon is already running
    if let Some(pid) = read_pid() {
        if is_process_alive(pid) {
            eprintln!("BitScout daemon is already running (PID {})", pid);
            std::process::exit(1);
        }
        // Stale pid file, remove it
        let _ = fs::remove_file(bitscout_dir().join("daemon.pid"));
    }

    let base = bitscout_dir();
    let shims_dir = base.join("shims");
    let socket_path = base.join("bitscout.sock");

    // Create directories
    fs::create_dir_all(&shims_dir).expect("Failed to create shims directory");

    // Find the bitscout-shim binary next to the current executable
    let exe_dir = std::env::current_exe()
        .expect("Failed to get current executable path")
        .parent()
        .expect("Failed to get executable directory")
        .to_path_buf();

    let shim_binary = exe_dir.join("bitscout-shim");
    if !shim_binary.exists() {
        eprintln!(
            "bitscout-shim binary not found at {:?}. Build it first with: cargo build -p bitscout-shim",
            shim_binary
        );
        std::process::exit(1);
    }

    // Copy shim binary as rg, grep, find, fd into shims dir
    let shim_names = ["rg", "grep", "find", "fd"];
    for name in &shim_names {
        let dest = shims_dir.join(name);
        if let Err(e) = fs::copy(&shim_binary, &dest) {
            eprintln!("Failed to copy shim as {}: {}", name, e);
            std::process::exit(1);
        }
        // Make executable (already is from copy, but ensure)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(0o755));
        }
    }

    // Resolve watch directory to absolute path
    let watch_path = std::fs::canonicalize(watch).unwrap_or_else(|_| PathBuf::from(watch));

    // Find the bitscout-daemon binary
    let daemon_binary = exe_dir.join("bitscout-daemon");
    if !daemon_binary.exists() {
        eprintln!(
            "bitscout-daemon binary not found at {:?}. Build it first with: cargo build -p bitscout-daemon",
            daemon_binary
        );
        std::process::exit(1);
    }

    // Launch the daemon process
    let child = Command::new(&daemon_binary)
        .arg("--watch")
        .arg(watch_path.to_str().unwrap_or("."))
        .arg("--socket")
        .arg(socket_path.to_str().unwrap())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    match child {
        Ok(child) => {
            let pid = child.id();
            // Write PID to daemon.pid
            let pid_path = base.join("daemon.pid");
            fs::write(&pid_path, pid.to_string()).expect("Failed to write daemon.pid");

            println!("BitScout daemon started (PID {})", pid);
            println!();
            println!("To intercept search commands, prepend the shims directory to your PATH:");
            println!();
            println!("  export PATH=\"{}:$PATH\"", shims_dir.display());
            println!();
            println!("Add this to your shell profile (.bashrc, .zshrc) for persistence.");
        }
        Err(e) => {
            eprintln!("Failed to start daemon: {}", e);
            std::process::exit(1);
        }
    }
}
