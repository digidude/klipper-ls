// Compile the tree-sitter grammar from grammar/ so the server parses
// exactly what the editor highlights.
use std::path::Path;

fn main() {
    let src = Path::new("grammar/src");
    cc::Build::new()
        .include(src)
        .file(src.join("parser.c"))
        .file(src.join("scanner.c"))
        .warnings(false)
        .compile("tree-sitter-klipper");

    for file in ["parser.c", "scanner.c"] {
        println!("cargo:rerun-if-changed={}", src.join(file).display());
    }
}
