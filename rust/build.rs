//! Build only Simon Tatham's unchanged, MIT-licensed puzzle engines.
//! All betterPod input, drawing callbacks and persistence stay in Rust.
use std::env;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let local = manifest.join("../src/puzzles");
    let source = env::var_os("BETTERPOD_PUZZLES_SOURCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if local.is_dir() {
                local
            } else {
                PathBuf::from("/repo/src/puzzles")
            }
        });
    let words = source.parent().unwrap().join("words");
    println!("cargo:rustc-env=BETTERPOD_WORDS_SOURCE={}", words.display());
    for file in ["answers.txt", "allowed.txt"] {
        println!("cargo:rerun-if-changed={}", words.join(file).display());
    }
    let files = [
        "midend", "drawing", "misc", "malloc", "random", "version", "tree234", "dsf", "divvy",
        "combi", "latin", "maxflow", "mines", "solo", "net", "flood", "samegame", "fifteen",
        "lightup", "unruly", "towers", "keen", "pattern", "tents",
    ];
    let mut build = cc::Build::new();
    build
        .include(&source)
        .define("COMBINED", None)
        .warnings(false);
    for file in files {
        let path = source.join(format!("{file}.c"));
        println!("cargo:rerun-if-changed={}", path.display());
        build.file(path);
    }
    for file in [
        "puzzles.h",
        "latin.h",
        "tree234.h",
        "maxflow.h",
        "version.h",
    ] {
        println!("cargo:rerun-if-changed={}", source.join(file).display());
    }
    println!("cargo:rerun-if-env-changed=BETTERPOD_PUZZLES_SOURCE");
    build.compile("betterpod_puzzles");
    if env::var("CARGO_CFG_TARGET_FAMILY").as_deref() == Ok("unix") {
        println!("cargo:rustc-link-lib=m");
    }
}
