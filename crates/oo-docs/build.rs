//! Declares the `kani` cfg so `#[cfg(kani)]` proof modules (80 section 3.2)
//! do not trip `unexpected_cfgs` in ordinary builds.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(kani)");
}
