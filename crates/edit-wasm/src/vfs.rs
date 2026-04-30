// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Virtual filesystem backed by browser `localStorage`.
//!
//! All files are stored under the key prefix `"vfs:"`.  For example, a file
//! named `notes.txt` is stored under the localStorage key `"vfs:notes.txt"`.

const VFS_PREFIX: &str = "vfs:";

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// Write `content` to the virtual file named `filename`.
///
/// Returns `true` on success, `false` if localStorage is unavailable or full.
pub fn write(filename: &str, content: &str) -> bool {
    let Some(storage) = local_storage() else {
        return false;
    };
    let key = format!("{VFS_PREFIX}{filename}");
    storage.set_item(&key, content).is_ok()
}

/// Read the content of the virtual file named `filename`.
///
/// Returns `None` if the file does not exist or localStorage is unavailable.
pub fn read(filename: &str) -> Option<String> {
    let storage = local_storage()?;
    let key = format!("{VFS_PREFIX}{filename}");
    storage.get_item(&key).ok()?
}

/// Returns `true` if a file named `filename` exists in the virtual filesystem.
pub fn exists(filename: &str) -> bool {
    let Some(storage) = local_storage() else {
        return false;
    };
    let key = format!("{VFS_PREFIX}{filename}");
    storage.get_item(&key).ok().flatten().is_some()
}

/// Return a sorted list of all filenames stored in the virtual filesystem.
pub fn list() -> Vec<String> {
    let Some(storage) = local_storage() else {
        return Vec::new();
    };
    let len = storage.length().unwrap_or(0);
    let mut files = Vec::new();
    for i in 0..len {
        if let Some(key) = storage.key(i).ok().flatten() {
            if let Some(name) = key.strip_prefix(VFS_PREFIX) {
                files.push(name.to_string());
            }
        }
    }
    files.sort();
    files
}
