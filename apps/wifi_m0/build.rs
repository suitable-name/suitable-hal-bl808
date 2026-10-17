use std::{env, fs, path::PathBuf};

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Copy memory.x to the build output directory
    fs::copy("memory.x", out_dir.join("memory.x")).unwrap();

    // Tell the linker where to find it
    println!("cargo:rustc-link-search={}", out_dir.display());

    // Re-run this script if memory.x changes
    println!("cargo:rerun-if-changed=memory.x");

    // Re-run if the build script itself changes
    println!("cargo:rerun-if-changed=build.rs");
}
