use std::{env, fs, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let engine = manifest.join("vendor/doomgeneric/doomgeneric");
    let makefile = fs::read_to_string(engine.join("Makefile.win")).expect(
        "DoomGeneric submodule missing; initialize it with git submodule update --init --recursive",
    );
    let object_list = makefile
        .lines()
        .find_map(|line| line.strip_prefix("SRC_DOOM ="))
        .expect("DoomGeneric Windows source list missing");

    let mut build = cc::Build::new();
    build
        .include(&engine)
        .include(manifest.join("native"))
        .define("SNDSERV", None)
        .define("DOOMGENERIC_RESX", "640")
        .define("DOOMGENERIC_RESY", "400")
        .warnings(false);
    if cfg!(windows) {
        build.define("strdup", "_strdup").define("mkdir", "_mkdir");
    }

    for object in object_list.split_whitespace() {
        let Some(stem) = object.strip_suffix(".o") else {
            continue;
        };
        if stem == "doomgeneric_win" {
            continue;
        }
        let source = engine.join(format!("{stem}.c"));
        if !source.exists() {
            panic!("DoomGeneric source file is missing: {}", source.display());
        }
        build.file(source);
        println!(
            "cargo:rerun-if-changed={}",
            engine.join(format!("{stem}.c")).display()
        );
    }
    build.file(manifest.join("native/doomgeneric_headless.c"));
    println!("cargo:rerun-if-changed=native/doomgeneric_headless.c");
    println!("cargo:rerun-if-changed=vendor/doomgeneric/doomgeneric/Makefile.win");

    // Prefer the standalone LLVM install on this workstation; cc-rs falls back
    // to the platform's configured C compiler on other machines.
    #[cfg(windows)]
    {
        let clang = PathBuf::from(r"C:\Program Files\LLVM\bin\clang.exe");
        if clang.exists() {
            build.compiler(clang);
        }
    }

    build.compile("doomgeneric_headless");
    if !cfg!(target_env = "msvc") {
        println!("cargo:rustc-link-lib=m");
    }
}
