use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=scripts/xwayland-lazy.c");
    println!("cargo:rerun-if-env-changed=CC");
    let dir = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("xwayland-helper");
    fs::create_dir_all(&dir).unwrap();
    let status = Command::new(env::var_os("CC").unwrap_or_else(|| "cc".into()))
        .args([
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            "scripts/xwayland-lazy.c",
            "-o",
        ])
        .arg(dir.join("Xwayland"))
        .status()
        .expect("C compiler required for the small Xwayland lazy starter");
    assert!(status.success(), "Xwayland lazy starter compilation failed");
    println!(
        "cargo:rustc-env=EMBER_XWAYLAND_HELPER_DIR={}",
        dir.display()
    );
}
