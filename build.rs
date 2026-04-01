use std::env;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    let host = env::var("HOST").unwrap_or_default();

    if target != host {
        println!("cargo:warning=Cross-compiling for target: {}", target);

        if target.contains("mips") || target.contains("mipsel") {
            println!("cargo:rustc-cfg=target_openwrt_mips");
        } else if target.contains("arm") {
            println!("cargo:rustc-cfg=target_openwrt_arm");
        } else if target.contains("aarch64") {
            println!("cargo:rustc-cfg=target_openwrt_aarch64");
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
}
