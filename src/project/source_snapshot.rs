// Copyright 2025-2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0 (the "License");

//! Immutable MIND source bytes shared by cache identity and native emission.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

#[cfg(test)]
thread_local! {
    static TEST_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(test)]
pub(crate) fn install_test_hook(hook: impl FnOnce() + 'static) {
    TEST_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(test)]
pub(crate) fn run_test_hook() {
    TEST_HOOK.with(|slot| {
        if let Some(hook) = slot.borrow_mut().take() {
            hook();
        }
    });
}

#[derive(Debug)]
struct CapturedFile {
    path: PathBuf,
    source: String,
}

/// One ordered, UTF-8-validated capture of every MIND translation-unit input.
#[derive(Debug)]
pub(crate) struct SourceSnapshot {
    files: Vec<CapturedFile>,
}

impl SourceSnapshot {
    pub(crate) fn capture(paths: &[PathBuf]) -> Result<Self> {
        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            let source = fs::read_to_string(path)
                .with_context(|| format!("cannot read project source {}", path.display()))?;
            files.push(CapturedFile {
                path: path.clone(),
                source,
            });
        }
        Ok(Self { files })
    }

    pub(crate) fn verify_paths(&self, paths: &[PathBuf]) -> Result<()> {
        if self.files.len() != paths.len()
            || self
                .files
                .iter()
                .zip(paths)
                .any(|(captured, resolved)| captured.path != *resolved)
        {
            return Err(anyhow!(
                "resolved project source set changed after source snapshot capture"
            ));
        }
        Ok(())
    }

    pub(crate) fn source(&self, path: &Path) -> Result<&str> {
        self.files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.source.as_str())
            .ok_or_else(|| anyhow!("source {} is absent from build snapshot", path.display()))
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.files
            .iter()
            .map(|file| (file.path.as_path(), file.source.as_str()))
    }
}
