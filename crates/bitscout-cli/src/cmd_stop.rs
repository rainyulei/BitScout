use std::fs;
use std::path::PathBuf;

/// Returns the ~/.bitscout directory path.
fn bitscout_dir() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join(".bitscout")
}

pub fn execute() {
    let pid_path = bitscout_dir().join("daemon.pid");

    let pid_str = match fs::read_to_string(&pid_path) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("No daemon.pid file found — daemon is not running.");
            std::process::exit(1);
        }
    };

    let pid: i32 = match pid_str.trim().parse() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Invalid PID in daemon.pid: {}", pid_str.trim());
            let _ = fs::remove_file(&pid_path);
            std::process::exit(1);
        }
    };

    // Send SIGTERM to the daemon process
    let ret = unsafe { libc::kill(pid, libc::SIGTERM) };

    if ret == 0 {
        println!("Sent SIGTERM to daemon (PID {})", pid);
    } else {
        let err = std::io::Error::last_os_error();
        eprintln!("Failed to send SIGTERM to PID {}: {}", pid, err);
    }

    // Remove the pid file regardless
    let _ = fs::remove_file(&pid_path);

    // Also clean up the socket file
    let socket_path = bitscout_dir().join("bitscout.sock");
    let _ = fs::remove_file(&socket_path);

    println!("BitScout daemon stopped.");
}
