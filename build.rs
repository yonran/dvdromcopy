extern crate pkg_config;

fn main() {
    pkg_config::Config::new().probe("libdvdcss").unwrap();
    println!("cargo::rerun-if-changed=build.rs");
}
