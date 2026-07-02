use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

/// Polls ~/.claude/projects/ for JSONL file changes every `interval` seconds.
/// Sends `true` via the channel when file modifications are detected.
/// The main thread should run incremental imports on receipt.
pub fn spawn_polling_thread(interval_secs: u64) -> mpsc::Receiver<bool> {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let mut watcher = FileWatcher::new();
        // First poll: baseline only, no signal
        watcher.poll_silent();

        loop {
            std::thread::sleep(Duration::from_secs(interval_secs));
            if watcher.poll() {
                // Channel send fails if receiver is dropped (app shutting down)
                if tx.send(true).is_err() {
                    break;
                }
            }
        }
    });

    rx
}

struct FileWatcher {
    /// Known files: path → mtime (unix seconds)
    known: HashMap<PathBuf, i64>,
}

impl FileWatcher {
    fn new() -> Self {
        FileWatcher { known: HashMap::new() }
    }

    /// Poll directory and record baseline without signalling
    fn poll_silent(&mut self) {
        let current = collect_jsonl_mtimes();
        self.known = current;
    }

    /// Poll directory, return true if any file changed, added, or removed
    fn poll(&mut self) -> bool {
        let current = collect_jsonl_mtimes();
        let mut changed = false;

        // Check for new or modified files
        for (path, mtime) in &current {
            match self.known.get(path) {
                Some(old) if old != mtime => changed = true,
                None => changed = true,
                _ => {}
            }
        }

        // Check for deleted files
        for path in self.known.keys() {
            if !current.contains_key(path) {
                changed = true;
                break;
            }
        }

        self.known = current;
        changed
    }
}

/// Recursively collect all .jsonl files under ~/.claude/projects/ with their mtimes
fn collect_jsonl_mtimes() -> HashMap<PathBuf, i64> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let projects_dir = PathBuf::from(&home).join(".claude/projects");
    let mut map = HashMap::new();
    if projects_dir.exists() {
        walk_dir(&projects_dir, &mut map);
    }
    map
}

fn walk_dir(dir: &PathBuf, out: &mut HashMap<PathBuf, i64>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk_dir(&path, out);
            } else if path.extension().map_or(false, |e| e == "jsonl") {
                let mtime = std::fs::metadata(&path)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                out.insert(path, mtime);
            }
        }
    }
}
