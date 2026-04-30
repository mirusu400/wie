fn main() {
    // WIE's ARM interpreter + JVM call chain easily overflows the Win64
    // default 1MB main-thread stack on real-world games. Bump the EXE's
    // stack reserve to 16MB on MSVC builds.
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows-msvc") {
        println!("cargo:rustc-link-arg-bin=wie_cli=/STACK:16777216");
    }
}
