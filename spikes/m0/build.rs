fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new().file("src/tap.m").flag("-fobjc-arc").flag("-mmacosx-version-min=14.2").compile("tap");
        for f in ["CoreAudio", "Foundation", "AudioToolbox"] {
            println!("cargo:rustc-link-lib=framework={f}");
        }
        println!("cargo:rerun-if-changed=src/tap.m");
    }
}
