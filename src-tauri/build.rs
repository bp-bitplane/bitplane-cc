fn main() {
    // Integration tests find the bundled daemon by its target-triple name.
    println!(
        "cargo:rustc-env=CC_TARGET_TRIPLE={}",
        std::env::var("TARGET").unwrap()
    );
    tauri_build::build()
}
