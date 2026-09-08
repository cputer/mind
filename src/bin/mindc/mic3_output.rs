// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Checked artifact emission for the compiler CLI.

use std::{fs, process};

use super::CompileArgs;

pub(super) fn emit_mic3_if_requested(
    cli: &CompileArgs,
    products: &libmind::pipeline::CompileProducts,
) {
    let path = match &cli.emit_mic3 {
        Some(p) => p,
        None => return,
    };
    let bytes = libmind::ir::compact::emit_mic3_checked(&products.ir).unwrap_or_else(|err| {
        eprintln!("error[emit-mic3]: cannot encode {path}: {err}");
        process::exit(1);
    });
    if let Err(err) = fs::write(path, &bytes) {
        eprintln!("error[emit-mic3]: failed to write {path}: {err}");
        process::exit(1);
    }
    eprintln!("Wrote mic@3 artifact: {path} ({} bytes)", bytes.len());
}
