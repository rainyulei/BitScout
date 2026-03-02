use std::os::unix::process::CommandExt;
use std::process::Command;

/// Execute the original command by searching PATH, skipping the shims directory.
/// This replaces the current process via exec() and never returns on success.
pub fn exec_original(cmd_name: &str, args: &[String]) -> ! {
    let shims_dir = dirs_shims();

    // Search PATH for the original binary, skipping the shims directory
    if let Some(path_env) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_env) {
            // Skip the shims directory to avoid infinite recursion
            if let Ok(canonical_dir) = std::fs::canonicalize(&dir) {
                if let Some(ref shims) = shims_dir {
                    if let Ok(canonical_shims) = std::fs::canonicalize(shims) {
                        if canonical_dir == canonical_shims {
                            continue;
                        }
                    }
                }
            }

            let candidate = dir.join(cmd_name);
            if candidate.is_file() {
                let err = Command::new(&candidate).args(args).exec();
                // exec() only returns on error
                eprintln!("bitscout: failed to exec {}: {}", candidate.display(), err);
                std::process::exit(127);
            }
        }
    }

    eprintln!("bitscout: command not found: {}", cmd_name);
    std::process::exit(127);
}

/// Returns the path to $HOME/.bitscout/shims/ if HOME is set.
fn dirs_shims() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|home| {
        std::path::PathBuf::from(home)
            .join(".bitscout")
            .join("shims")
    })
}
