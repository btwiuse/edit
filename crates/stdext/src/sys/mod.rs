// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Platform abstractions.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;
#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(not(any(windows, target_arch = "wasm32")))]
pub use std::fs::canonicalize;

#[cfg(unix)]
pub use unix::*;
#[cfg(windows)]
pub use windows::*;
#[cfg(target_arch = "wasm32")]
pub use wasm::*;
