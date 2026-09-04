//! Builds the Ring 3 userspace programs and hands their ELF paths to the kernel.
//!
//! The kernel embeds userspace binaries with `include_bytes!`, so they have to
//! exist before the kernel compiles. Doing it here keeps a plain `cargo build`
//! working, rather than requiring everyone to run a shell script in the right
//! order first.
//!
//! Two details keep this from tripping over itself:
//!
//! * A separate `--target-dir`. Nested cargo sharing a target directory blocks
//!   on the parent's lock and deadlocks.
//! * `CARGO_ENCODED_RUSTFLAGS` set explicitly. `.cargo/config.toml` at the repo
//!   root builds for the kernel -- higher-half linker script, `code-model=kernel`
//!   -- and a config file applies to anything built underneath it. The
//!   environment variable takes precedence over config files, which is the only
//!   reliable way to override the whole set.

use std::path::PathBuf;
use std::process::Command;

/// Userspace binaries to build, as (crate directory, binary name). All live in
/// one crate so they can share `lib.rs`; each is a separate `[[bin]]` target.
const USER_PROGRAMS: &[(&str, &str)] = &[
    ("userspace", "aegis_terminal"),
    ("userspace", "aegis_crash_test"),
];

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());

    println!("cargo:rerun-if-changed=build.rs");

    for (dir, bin) in USER_PROGRAMS {
        let crate_dir = manifest_dir.join(dir);
        let linker_script = crate_dir.join("linker.ld");
        let manifest = crate_dir.join("Cargo.toml");
        let target_dir = manifest_dir.join("target").join("userspace");

        println!("cargo:rerun-if-changed={}", crate_dir.join("src").display());
        println!("cargo:rerun-if-changed={}", linker_script.display());
        println!("cargo:rerun-if-changed={}", manifest.display());

        // Userspace is a flat lower-half program: `code-model=small`, its own
        // linker script, and the same SSE ban as the kernel -- the scheduler
        // saves general-purpose registers only, so any FPU or vector state a
        // task kept across a context switch would be silently corrupted.
        let rustflags = [
            "-C".to_string(),
            format!("link-arg=-T{}", linker_script.display()),
            "-C".to_string(),
            "relocation-model=static".to_string(),
            "-C".to_string(),
            "code-model=small".to_string(),
            "-C".to_string(),
            "no-redzone=y".to_string(),
            "-C".to_string(),
            "target-feature=-sse,-sse2,-avx,-avx2".to_string(),
        ]
        .join("\u{1f}");

        let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        cmd.args([
            "build",
            "--release",
            "--target",
            "x86_64-unknown-none",
            "--bin",
            bin,
            "--manifest-path",
        ])
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target_dir)
        .env("CARGO_ENCODED_RUSTFLAGS", rustflags)
        // Inherited flags would otherwise be layered on top of ours.
        .env_remove("RUSTFLAGS")
        // Nested cargo must not join the parent's jobserver; it has its own.
        .env_remove("CARGO_MAKEFLAGS");

        let status = cmd
            .status()
            .unwrap_or_else(|e| panic!("failed to invoke cargo for userspace crate '{dir}': {e}"));
        if !status.success() {
            panic!("userspace crate '{dir}' failed to build ({status})");
        }

        let elf = target_dir
            .join("x86_64-unknown-none")
            .join("release")
            .join(bin);
        if !elf.exists() {
            panic!("userspace binary not found at {}", elf.display());
        }

        // Uppercased binary name becomes the env var the kernel reads, e.g.
        // AEGIS_USER_TERMINAL_ELF.
        println!(
            "cargo:rustc-env={}_ELF={}",
            bin.to_uppercase(),
            elf.display()
        );
    }
}
