// Windows gives the main thread 1 MiB of stack, where Linux and macOS give
// 8. pepe runs on that thread (a current-thread runtime), and a debug build
// overflowed it building the command line, before parsing anything. Ask the
// linker for the same 8 MiB the others have. Set here rather than in
// .cargo/config.toml so `cargo install pepe` gets it too.
fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" {
        match target_env.as_str() {
            "msvc" => println!("cargo:rustc-link-arg-bins=/STACK:8388608"),
            "gnu" => println!("cargo:rustc-link-arg-bins=-Wl,--stack,8388608"),
            _ => {}
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
}
