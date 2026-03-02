mod cmd_memory;
mod cmd_start;
mod cmd_status;
mod cmd_stop;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bitscout", about = "SIMD-accelerated search daemon for AI Agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the BitScout daemon
    Start {
        /// Directory to watch for file changes
        #[arg(long, default_value = ".")]
        watch: String,
    },
    /// Stop the BitScout daemon
    Stop,
    /// Show daemon status
    Status,
    /// Manage persistent memory entries
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
}

#[derive(Subcommand)]
enum MemoryAction {
    /// Save a key-value memory entry
    Save {
        /// Key name for the entry
        key: String,
        /// Content to store
        content: String,
    },
    /// Remove a memory entry by key
    Remove {
        /// Key name to remove
        key: String,
    },
    /// Search memory entries by query
    Search {
        /// Search query (matched against keys and content)
        query: String,
    },
    /// List all memory entries
    List,
    /// Clear all memory entries
    Clear,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start { watch } => cmd_start::execute(&watch),
        Commands::Stop => cmd_stop::execute(),
        Commands::Status => cmd_status::execute(),
        Commands::Memory { action } => match action {
            MemoryAction::Save { key, content } => cmd_memory::save(&key, &content),
            MemoryAction::Remove { key } => cmd_memory::remove(&key),
            MemoryAction::Search { query } => cmd_memory::search(&query),
            MemoryAction::List => cmd_memory::list(),
            MemoryAction::Clear => cmd_memory::clear(),
        },
    }
}
