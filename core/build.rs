fn main() {
    let mut build = cc::Build::new();
    build
        .file("native/effects/bridge.c")
        .std("c11")
        .opt_level(2)
        .warnings_into_errors(true);
    if build.get_compiler().is_like_msvc() {
        build.flag("/fp:strict");
    } else {
        build.flag("-ffp-contract=off").flag("-fno-fast-math");
        println!("cargo:rustc-link-lib=m");
    }
    build.compile("cumulus_effects");
    println!("cargo:rerun-if-changed=native/effects/bridge.c");
    println!("cargo:rerun-if-changed=native/effects/netease_dsp.c");
    println!("cargo:rerun-if-changed=native/effects/netease_multiband.c");
    println!("cargo:rerun-if-changed=native/effects/netease_rotate.c");
}
