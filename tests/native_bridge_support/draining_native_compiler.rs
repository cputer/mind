//! Host-native compiler fixture for admission-only controls.
//!
//! The committed stage1.elf is a Linux x86-64 executable. These tests exercise
//! source admission and image transport on every CI host, so they must not hand
//! an aarch64/macOS or Windows runner an ELF it cannot execute. Read stdin to
//! EOF before writing 397 fixed bytes with ELF magic; emitting first
//! would recreate the Broken-pipe false failure this fixture is meant to expose.
//! This output is not an executable ELF and is not native code-generation proof.

use std::fs::File;
use std::io::{self, Read, Write};

fn main() {
    let mut image = Vec::new();
    io::stdin()
        .read_to_end(&mut image)
        .expect("read complete native image");
    let capture =
        std::env::var_os("MIND_NATIVE_TEST_CAPTURE").expect("MIND_NATIVE_TEST_CAPTURE is required");
    File::create(capture)
        .expect("create native image capture")
        .write_all(&image)
        .expect("write native image capture");

    let mut stdout = io::stdout().lock();
    stdout.write_all(b"\x7fELF").expect("write ELF magic");
    stdout
        .write_all(&[0; 393])
        .expect("write deterministic ELF anchor");
}
