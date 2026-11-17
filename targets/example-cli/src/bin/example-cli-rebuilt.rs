//! The stand's program, built a second time. It behaves as `example-cli` does
//! and its file differs, so a test can replace the program with another build.

fn main() {
    example_cli::program::main(example_cli::Build::Rebuilt)
}
