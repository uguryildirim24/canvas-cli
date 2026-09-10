fn main() {
    println!(
        "cargo:rustc-env=CANVAS_BUILD_TARGET={}",
        std::env::var("TARGET").expect("Cargo sets TARGET")
    );
}
