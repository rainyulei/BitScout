use bitscout_memory::store::MemoryStore;
use std::path::PathBuf;

/// Returns the memory entries directory: ~/.bitscout/memory/entries/
fn memory_dir() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home)
        .join(".bitscout")
        .join("memory")
        .join("entries")
}

fn get_store() -> MemoryStore {
    MemoryStore::new(&memory_dir()).expect("Failed to initialize memory store")
}

pub fn save(key: &str, content: &str) {
    let store = get_store();
    match store.save(key, content) {
        Ok(()) => println!("Saved memory entry: {}", key),
        Err(e) => eprintln!("Failed to save entry '{}': {}", key, e),
    }
}

pub fn remove(key: &str) {
    let store = get_store();
    match store.remove(key) {
        Ok(()) => println!("Removed memory entry: {}", key),
        Err(e) => eprintln!("Failed to remove entry '{}': {}", key, e),
    }
}

pub fn search(query: &str) {
    let store = get_store();
    let results = store.search(query);

    if results.is_empty() {
        println!("No entries matching '{}'", query);
        return;
    }

    println!("Found {} entries matching '{}':", results.len(), query);
    for entry in &results {
        println!();
        println!("  [{}]", entry.key);
        println!("  {}", entry.content);
    }
}

pub fn list() {
    let store = get_store();
    let keys = store.list();

    if keys.is_empty() {
        println!("No memory entries stored.");
        return;
    }

    println!("Memory entries ({}):", keys.len());
    for key in &keys {
        // Also show the content preview
        if let Some(entry) = store.get(key) {
            let preview: String = entry.content.chars().take(80).collect();
            let ellipsis = if entry.content.len() > 80 { "..." } else { "" };
            println!("  [{}] {}{}", key, preview, ellipsis);
        } else {
            println!("  [{}]", key);
        }
    }
}

pub fn clear() {
    let store = get_store();
    match store.clear() {
        Ok(()) => println!("All memory entries cleared."),
        Err(e) => eprintln!("Failed to clear entries: {}", e),
    }
}
