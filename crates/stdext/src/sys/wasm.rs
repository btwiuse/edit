// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! WASM platform abstractions for virtual memory.
//!
//! In WASM there is no `mmap`, so we simulate reserve/commit/release
//! using the standard heap allocator.  Since WASM is single-threaded and
//! the WASM linear memory never shrinks, allocating the full reserved size
//! up-front is the simplest correct approach.

use std::alloc::{self, Layout};
use std::io;
use std::ptr::NonNull;

/// Reserves (and immediately fully commits) a virtual memory region of the
/// given size.  On WASM there is no OS-level distinction between reserved and
/// committed pages, so we allocate everything at once.
///
/// # Safety
///
/// This function is unsafe because it returns a raw pointer.
/// Release the memory with [`virtual_release`] when done.
pub unsafe fn virtual_reserve(size: usize) -> io::Result<NonNull<u8>> {
    // Use 8-byte alignment, which is sufficient for all arena uses.
    let layout = Layout::from_size_align(size, 8)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid size"))?;
    let ptr = unsafe { alloc::alloc_zeroed(layout) };
    NonNull::new(ptr).ok_or_else(|| io::Error::new(io::ErrorKind::OutOfMemory, "allocation failed"))
}

/// No-op on WASM: memory is already accessible after [`virtual_reserve`].
///
/// # Safety
///
/// The pointer and size must match a previous [`virtual_reserve`] call.
pub unsafe fn virtual_commit(_base: NonNull<u8>, _size: usize) -> io::Result<()> {
    Ok(())
}

/// Releases a virtual memory region previously obtained from [`virtual_reserve`].
///
/// # Safety
///
/// The pointer and size must exactly match a previous [`virtual_reserve`] call.
pub unsafe fn virtual_release(base: NonNull<u8>, size: usize) {
    // Guard against zero-sized releases (NonNull::dangling() case).
    if size == 0 {
        return;
    }
    let layout = Layout::from_size_align(size, 8).expect("invalid layout in virtual_release");
    unsafe { alloc::dealloc(base.as_ptr(), layout) };
}
