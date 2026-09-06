// Copyright 2026 STARGA Inc.
// Licensed under the Apache License, Version 2.0.

//! Serializes a project's shared objects, manifest edits, cleanup, and cache
//! publication. Other projects remain independent. The persistent lock file
//! lives outside `target/`: cleanup removes that whole directory, and unlinking
//! a held lock lets a later process lock a different inode.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

pub(crate) struct ProjectBuildLock {
    _file: File,
    root: PathBuf,
}

impl ProjectBuildLock {
    pub(crate) fn acquire(root: &Path) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".mind-build.lock"))?;
        // Qualified trait call preserves the declared Rust 1.85 MSRV.
        fs2::FileExt::lock_exclusive(&file)?;
        Ok(Self {
            _file: file,
            root: root.to_path_buf(),
        })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}

/// Restores an exact manifest snapshot on every normal or unwinding exit.
///
/// Callers still invoke [`restore`](Self::restore) explicitly so an I/O failure
/// is surfaced. `Drop` is the panic/error backstop; it deliberately retries a
/// failed explicit restore instead of leaving a temporary entry behind without
/// another attempt.
pub(crate) struct ManifestEdit {
    path: PathBuf,
    original: Option<Vec<u8>>,
    restore_on_drop: bool,
}

impl ManifestEdit {
    pub(crate) fn replace(
        path: &Path,
        original: Option<Vec<u8>>,
        replacement: &[u8],
    ) -> io::Result<Self> {
        let mut edit = Self {
            path: path.to_path_buf(),
            original,
            restore_on_drop: true,
        };
        if let Err(write_error) = fs::write(path, replacement) {
            let restore_error = edit.restore_inner().err();
            if restore_error.is_none() {
                edit.restore_on_drop = false;
            }
            let detail = match restore_error {
                Some(error) => format!("; restoring the manifest also failed: {error}"),
                None => "; the original manifest was restored".to_string(),
            };
            return Err(io::Error::new(
                write_error.kind(),
                format!("cannot write temporary manifest: {write_error}{detail}"),
            ));
        }
        Ok(edit)
    }

    pub(crate) fn restore(mut self) -> io::Result<()> {
        let result = self.restore_inner();
        if result.is_ok() {
            self.restore_on_drop = false;
        }
        result
    }

    fn restore_inner(&self) -> io::Result<()> {
        match &self.original {
            Some(bytes) => fs::write(&self.path, bytes),
            None => match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            },
        }
    }
}

impl Drop for ManifestEdit {
    fn drop(&mut self) {
        if self.restore_on_drop {
            let _ = self.restore_inner();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ManifestEdit, ProjectBuildLock};
    use std::fs;
    use std::io;
    use std::path::Path;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn same_project_waits_for_the_existing_transaction() {
        let root = tempfile::tempdir().unwrap();
        let first = ProjectBuildLock::acquire(root.path()).unwrap();
        let path = root.path().to_path_buf();
        let (tx, rx) = mpsc::channel();
        let waiter = thread::spawn(move || {
            let lock = ProjectBuildLock::acquire(&path).unwrap();
            tx.send(()).unwrap();
            drop(lock);
        });

        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        drop(first);
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        waiter.join().unwrap();
    }

    #[test]
    fn different_projects_do_not_share_a_lock() {
        let first_root = tempfile::tempdir().unwrap();
        let second_root = tempfile::tempdir().unwrap();
        let first = ProjectBuildLock::acquire(first_root.path()).unwrap();

        let second = ProjectBuildLock::acquire(second_root.path()).unwrap();

        drop(second);
        drop(first);
    }

    #[test]
    fn an_error_releases_the_transaction() {
        fn fail_while_locked(root: &Path) -> io::Result<()> {
            let _lock = ProjectBuildLock::acquire(root)?;
            Err(io::Error::other("injected build failure"))
        }

        let root = tempfile::tempdir().unwrap();
        assert!(fail_while_locked(root.path()).is_err());
        let replacement = ProjectBuildLock::acquire(root.path()).unwrap();
        drop(replacement);
    }

    #[test]
    fn target_cleanup_cannot_replace_the_held_lock_inode() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("target")).unwrap();
        let first = ProjectBuildLock::acquire(root.path()).unwrap();
        fs::remove_dir_all(root.path().join("target")).unwrap();
        let path = root.path().to_path_buf();
        let (tx, rx) = mpsc::channel();
        let waiter = thread::spawn(move || {
            let lock = ProjectBuildLock::acquire(&path).unwrap();
            tx.send(()).unwrap();
            drop(lock);
        });

        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        drop(first);
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        waiter.join().unwrap();
    }

    #[test]
    fn manifest_edit_restores_exact_bytes_during_unwind() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("Mind.toml");
        let original = b"[build]\r\nentry='src/main.mind'\r\n";
        fs::write(&path, original).unwrap();
        let _lock = ProjectBuildLock::acquire(root.path()).unwrap();

        let result = std::panic::catch_unwind(|| {
            let _edit = ManifestEdit::replace(
                &path,
                Some(original.to_vec()),
                b"[build]\nentry = \"tests/probe.mind\"\n",
            )
            .unwrap();
            panic!("injected compiler panic");
        });

        assert!(result.is_err());
        assert_eq!(fs::read(path).unwrap(), original);
    }
}
