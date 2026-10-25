//! Builds LibAtrac9 (libatrac9/, MIT) into a static library. It needs only the C standard library.

fn main() {
    let sources: Vec<_> = std::fs::read_dir("libatrac9")
        .expect("the vendored LibAtrac9 sources")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|e| e == "c"))
        .collect();
    for path in &sources {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!("cargo:rerun-if-changed=libatrac9");
    cc::Build::new()
        .files(&sources)
        .include("libatrac9")
        // Without it MSVC marks the API dllimport; we link it statically.
        .define("COMPILING_DLL", None)
        .opt_level(2)
        .warnings(false)
        .compile("atrac9");
}
