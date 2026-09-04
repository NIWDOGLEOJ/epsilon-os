//! In-Memory Virtual Filesystem (RAM Disk VFS) for AegisOS
//!
//! Provides thread-safe, hierarchical in-memory file storage with Inodes,
//! pre-seeded system documentation, and full CRUD operations.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

use crate::arch::InterruptGuard;

/// Inode representing an individual file or directory in the RAM disk VFS
#[derive(Clone)]
pub struct VfsNode {
    pub path: String,
    pub data: Vec<u8>,
    pub is_directory: bool,
    pub created_tick: u64,
    pub modified_tick: u64,
}

/// Metadata summary returned by `list_dir`
#[derive(Clone)]
pub struct FileMetadata {
    pub path: String,
    pub name: String,
    pub size_bytes: usize,
    pub is_directory: bool,
}

/// In-Memory RAM Disk Filesystem Container
pub struct RamFs {
    nodes: Vec<VfsNode>,
}

impl RamFs {
    pub const fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Initializes pre-seeded system directories and documentation files.
    pub fn init_seed_files(&mut self) {
        if !self.nodes.is_empty() {
            return;
        }

        // Directories
        self.nodes.push(VfsNode {
            path: "/".to_string(),
            data: Vec::new(),
            is_directory: true,
            created_tick: 0,
            modified_tick: 0,
        });
        self.nodes.push(VfsNode {
            path: "/system".to_string(),
            data: Vec::new(),
            is_directory: true,
            created_tick: 0,
            modified_tick: 0,
        });
        self.nodes.push(VfsNode {
            path: "/user".to_string(),
            data: Vec::new(),
            is_directory: true,
            created_tick: 0,
            modified_tick: 0,
        });

        // /welcome.txt
        let welcome_content = "Welcome to AegisPad on AegisOS!\n\n\
Key Features:\n\
1. Ring 0 / Ring 3 hardware memory isolation\n\
2. Crash-resilient fault recovery without desktop freezes\n\
3. macOS-inspired double-buffered 60 FPS compositor\n\
4. Ultralight memory footprint (< 60MB RAM at idle)\n\
5. In-memory RAM disk Virtual Filesystem (VFS)\n\n\
Type anywhere to edit, and click [ Save ] to persist to /welcome.txt!\n";

        self.nodes.push(VfsNode {
            path: "/welcome.txt".to_string(),
            data: welcome_content.as_bytes().to_vec(),
            is_directory: false,
            created_tick: 0,
            modified_tick: 0,
        });

        // /system/readme.txt
        let readme_content = "=== AegisOS Architecture Overview ===\n\
- Kernel: x86_64 bare-metal Rust (no_std)\n\
- Bootloader: Limine protocol with higher-half direct mapping (HHDM)\n\
- Isolation: Private PML4 address spaces with Ring 3 fault isolation\n\
- Graphics: 1280x800 linear framebuffer with 60 FPS frame pacing\n\
- Multitasking: 100Hz preemptive round-robin scheduler with deferred zombie reaping\n";

        self.nodes.push(VfsNode {
            path: "/system/readme.txt".to_string(),
            data: readme_content.as_bytes().to_vec(),
            is_directory: false,
            created_tick: 0,
            modified_tick: 0,
        });

        // /system/os_release
        let os_release_content = "NAME=AegisOS\n\
VERSION=0.1.0\n\
ARCH=x86_64\n\
AUTHOR=AegisOS Core Team\n";

        self.nodes.push(VfsNode {
            path: "/system/os_release".to_string(),
            data: os_release_content.as_bytes().to_vec(),
            is_directory: false,
            created_tick: 0,
            modified_tick: 0,
        });

        // /user/notes.txt
        let notes_content = "AegisOS User Notes:\n\
- Try typing 'symbols' in Terminal for unicode typography.\n\
- Try typing 'calc 16 * 4' for inline arithmetic.\n\
- Try typing 'free' for physical and heap memory telemetry.\n";

        self.nodes.push(VfsNode {
            path: "/user/notes.txt".to_string(),
            data: notes_content.as_bytes().to_vec(),
            is_directory: false,
            created_tick: 0,
            modified_tick: 0,
        });
    }

    /// Normalizes path strings to standard `/path/to/file` form.
    fn normalize_path(path: &str) -> String {
        let trimmed = path.trim();
        if trimmed.is_empty() || trimmed == "/" {
            return "/".to_string();
        }
        if trimmed.starts_with('/') {
            trimmed.to_string()
        } else {
            format!("/{}", trimmed)
        }
    }

    /// Checks if a file exists at the given path.
    pub fn exists(&self, path: &str) -> bool {
        let norm = Self::normalize_path(path);
        self.nodes.iter().any(|n| n.path == norm)
    }

    /// Reads raw file bytes.
    pub fn read(&self, path: &str) -> Result<Vec<u8>, &'static str> {
        let norm = Self::normalize_path(path);
        self.nodes
            .iter()
            .find(|n| n.path == norm)
            .map(|n| n.data.clone())
            .ok_or("File not found")
    }

    /// Writes data to a file, creating it if it doesn't already exist.
    pub fn write(&mut self, path: &str, data: &[u8]) -> Result<(), &'static str> {
        let norm = Self::normalize_path(path);
        let tick = crate::task::get_uptime_ticks();

        if let Some(node) = self.nodes.iter_mut().find(|n| n.path == norm) {
            if node.is_directory {
                return Err("Cannot write data to a directory");
            }
            node.data = data.to_vec();
            node.modified_tick = tick;
            Ok(())
        } else {
            self.nodes.push(VfsNode {
                path: norm,
                data: data.to_vec(),
                is_directory: false,
                created_tick: tick,
                modified_tick: tick,
            });
            Ok(())
        }
    }

    /// Creates a directory in the VFS.
    pub fn mkdir(&mut self, path: &str) -> Result<(), &'static str> {
        let norm = Self::normalize_path(path);
        if norm == "/" {
            return Err("Root directory already exists");
        }
        if self.nodes.iter().any(|n| n.path == norm) {
            return Err("Directory or file already exists");
        }
        let tick = crate::task::get_uptime_ticks();
        self.nodes.push(VfsNode {
            path: norm,
            data: Vec::new(),
            is_directory: true,
            created_tick: tick,
            modified_tick: tick,
        });
        Ok(())
    }

    /// Removes a file from the VFS.
    pub fn remove(&mut self, path: &str) -> Result<(), &'static str> {
        let norm = Self::normalize_path(path);
        if norm == "/" {
            return Err("Cannot remove root directory");
        }
        if let Some(pos) = self.nodes.iter().position(|n| n.path == norm) {
            self.nodes.remove(pos);
            Ok(())
        } else {
            Err("File not found")
        }
    }

    /// Lists files and subdirectories directly inside `dir_path`.
    pub fn list(&self, dir_path: &str) -> Vec<FileMetadata> {
        let norm_dir = Self::normalize_path(dir_path);
        let mut results = Vec::new();

        for node in &self.nodes {
            if node.path == norm_dir || node.path == "/" {
                continue;
            }

            let matches = if norm_dir == "/" {
                // Root children have exactly one leading slash and no other slashes
                let sub = &node.path[1..];
                !sub.contains('/')
            } else {
                let prefix = format!("{}/", norm_dir);
                if node.path.starts_with(&prefix) {
                    let sub = &node.path[prefix.len()..];
                    !sub.contains('/')
                } else {
                    false
                }
            };

            if matches {
                let name = node.path.split('/').last().unwrap_or("").to_string();
                results.push(FileMetadata {
                    path: node.path.clone(),
                    name,
                    size_bytes: node.data.len(),
                    is_directory: node.is_directory,
                });
            }
        }

        results
    }

    /// Returns list of all file paths currently stored in the VFS.
    pub fn all_files(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|n| !n.is_directory)
            .map(|n| n.path.clone())
            .collect()
    }

    /// Returns list of all file and directory paths in the VFS.
    pub fn all_paths(&self) -> Vec<String> {
        self.nodes.iter().map(|n| n.path.clone()).collect()
    }

    /// Returns (total_files, total_bytes).
    pub fn stats(&self) -> (usize, usize) {
        let count = self.nodes.iter().filter(|n| !n.is_directory).count();
        let bytes = self.nodes.iter().map(|n| n.data.len()).sum();
        (count, bytes)
    }
}

/// Global In-Memory Virtual Filesystem
pub static VFS: Mutex<RamFs> = Mutex::new(RamFs::new());

/// Initializes the global VFS with seed directories and documents.
pub fn init_vfs() {
    let _guard = InterruptGuard::acquire();
    VFS.lock().init_seed_files();
    crate::serial_println!("[OK] In-Memory Virtual Filesystem (RAM Disk VFS) initialized.");
}

/// Reads file contents as a UTF-8 String.
pub fn read_to_string(path: &str) -> Result<String, &'static str> {
    let _guard = InterruptGuard::acquire();
    let bytes = VFS.lock().read(path)?;
    String::from_utf8(bytes).map_err(|_| "Invalid UTF-8 encoding")
}

/// Reads raw binary bytes from a file.
pub fn read_file(path: &str) -> Result<Vec<u8>, &'static str> {
    let _guard = InterruptGuard::acquire();
    VFS.lock().read(path)
}

/// Writes bytes to a file.
pub fn write_file(path: &str, data: &[u8]) -> Result<(), &'static str> {
    let _guard = InterruptGuard::acquire();
    VFS.lock().write(path, data)
}

/// Removes a file.
pub fn remove_file(path: &str) -> Result<(), &'static str> {
    let _guard = InterruptGuard::acquire();
    VFS.lock().remove(path)
}

/// Creates a new directory.
pub fn create_dir(path: &str) -> Result<(), &'static str> {
    let _guard = InterruptGuard::acquire();
    VFS.lock().mkdir(path)
}

/// Checks if a file exists.
pub fn file_exists(path: &str) -> bool {
    let _guard = InterruptGuard::acquire();
    VFS.lock().exists(path)
}

/// Lists files in a directory.
pub fn list_dir(path: &str) -> Vec<FileMetadata> {
    let _guard = InterruptGuard::acquire();
    VFS.lock().list(path)
}

/// Returns (total_file_count, total_bytes_used).
pub fn get_fs_stats() -> (usize, usize) {
    let _guard = InterruptGuard::acquire();
    VFS.lock().stats()
}

/// Returns a list of all non-directory file paths.
pub fn get_all_file_paths() -> Vec<String> {
    let _guard = InterruptGuard::acquire();
    VFS.lock().all_files()
}

/// Returns a list of all file and directory paths.
pub fn get_all_vfs_paths() -> Vec<String> {
    let _guard = InterruptGuard::acquire();
    VFS.lock().all_paths()
}
