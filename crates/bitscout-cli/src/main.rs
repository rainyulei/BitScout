mod cmd_memory;

use bitscout_core::dispatch::{self, FALLBACK_EXIT_CODE};
use clap::{Parser, Subcommand};
use std::env;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process;

/// Detect if invoked via symlink as rg/grep/find/fd/cat.
fn detect_busybox_command() -> Option<String> {
    let argv0 = env::args().next()?;
    let name = Path::new(&argv0)
        .file_name()?
        .to_str()?
        .to_string();

    match name.as_str() {
        "rg" | "grep" | "find" | "fd" | "cat" => Some(name),
        _ => None,
    }
}

/// BusyBox mode: dispatch as rg/grep/find/fd/cat, fallback to real binary on failure.
fn run_busybox(command: &str) -> ! {
    let args: Vec<String> = env::args().skip(1).collect();
    let cwd = env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .to_string();

    let resp = dispatch::dispatch(command, &args, &cwd);

    if resp.exit_code == FALLBACK_EXIT_CODE {
        // Fallback to real binary
        exec_original(command);
    }

    if !resp.stdout.is_empty() {
        print!("{}", resp.stdout);
    }
    if !resp.stderr.is_empty() {
        eprint!("{}", resp.stderr);
    }
    process::exit(resp.exit_code);
}

/// exec() the real binary, searching PATH but skipping our own directory.
fn exec_original(command: &str) -> ! {
    let self_dir = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    let path_var = env::var("PATH").unwrap_or_default();
    for dir in path_var.split(':') {
        // Skip our own directory to avoid infinite recursion
        if let Some(ref sd) = self_dir {
            if let Ok(canon) = std::fs::canonicalize(dir) {
                if let Ok(self_canon) = std::fs::canonicalize(sd) {
                    if canon == self_canon {
                        continue;
                    }
                }
            }
        }

        let candidate = PathBuf::from(dir).join(command);
        if candidate.is_file() {
            let err = std::process::Command::new(&candidate)
                .args(env::args().skip(1))
                .exec();
            // exec() only returns on error
            eprintln!("bitscout: exec {}: {}", candidate.display(), err);
            process::exit(127);
        }
    }

    eprintln!("bitscout: {}: command not found in PATH", command);
    process::exit(127);
}

// ---------------------------------------------------------------------------
// Subcommand mode (invoked as `bitscout <subcommand>`)
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "bitscout", about = "SIMD-accelerated search toolkit for AI Agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Search files using BitScout engine
    Search {
        /// Search pattern
        pattern: String,
        /// Directory to search (default: current directory)
        #[arg(default_value = ".")]
        path: String,
        /// Use semantic (Random Projection) scoring
        #[arg(long)]
        semantic: bool,
    },
    /// Manage persistent memory entries
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
    /// Install symlinks (rg, grep, find, fd, cat → bitscout)
    Install {
        /// Target directory for symlinks (default: ~/.bitscout/shims)
        #[arg(long)]
        dir: Option<String>,
    },
}

#[derive(Subcommand)]
enum MemoryAction {
    /// Save a key-value memory entry
    Save {
        key: String,
        content: String,
    },
    /// Remove a memory entry by key
    Remove {
        key: String,
    },
    /// Search memory entries by query
    Search {
        query: String,
    },
    /// List all memory entries
    List,
    /// Clear all memory entries
    Clear,
}

fn main() {
    // Check BusyBox mode first (argv[0] is rg/grep/find/fd/cat)
    if let Some(command) = detect_busybox_command() {
        run_busybox(&command);
    }

    // Normal subcommand mode
    let cli = Cli::parse();

    match cli.command {
        Commands::Search {
            pattern,
            path,
            semantic,
        } => {
            let cwd = env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .to_string_lossy()
                .to_string();

            let mut args = vec![pattern, path];
            if semantic {
                args.insert(0, "--semantic".to_string());
            }

            let resp = dispatch::dispatch("rg", &args, &cwd);
            if !resp.stdout.is_empty() {
                print!("{}", resp.stdout);
            }
            if !resp.stderr.is_empty() {
                eprint!("{}", resp.stderr);
            }
            process::exit(resp.exit_code);
        }
        Commands::Memory { action } => match action {
            MemoryAction::Save { key, content } => cmd_memory::save(&key, &content),
            MemoryAction::Remove { key } => cmd_memory::remove(&key),
            MemoryAction::Search { query } => cmd_memory::search(&query),
            MemoryAction::List => cmd_memory::list(),
            MemoryAction::Clear => cmd_memory::clear(),
        },
        Commands::Install { dir } => {
            install_symlinks(dir.as_deref());
        }
    }
}

fn install_symlinks(target_dir: Option<&str>) {
    let home = env::var("HOME").expect("HOME not set");
    let dir = match target_dir {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(&home).join(".bitscout").join("shims"),
    };

    std::fs::create_dir_all(&dir).expect("Failed to create shims directory");

    let self_exe = env::current_exe().expect("Cannot determine own path");
    let commands = ["rg", "grep", "find", "fd", "cat"];

    for cmd in &commands {
        let link = dir.join(cmd);
        // Remove existing symlink if present
        if link.exists() || link.symlink_metadata().is_ok() {
            let _ = std::fs::remove_file(&link);
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&self_exe, &link)
                .unwrap_or_else(|e| eprintln!("Failed to create symlink {}: {}", link.display(), e));
        }
    }

    println!("Installed symlinks in {}", dir.display());
    println!();
    println!("Add to your shell profile:");
    println!("  export PATH=\"{}:$PATH\"", dir.display());
}
