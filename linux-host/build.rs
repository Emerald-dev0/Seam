fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = std::path::Path::new(&out_dir);
    let link_path = out_path.join("libcrypto.so");
    if !link_path.exists() {
        let _ = std::os::unix::fs::symlink("/usr/lib/x86_64-linux-gnu/libcrypto.so.3", &link_path);
    }
    println!("cargo:rustc-link-search=native={}", out_dir);

    println!("cargo:rustc-link-lib=X11");
    println!("cargo:rustc-link-lib=Xfixes");
    println!("cargo:rustc-link-lib=Xi");
    println!("cargo:rustc-link-lib=Xrandr");
    println!("cargo:rustc-link-lib=Xtst");
    println!("cargo:rustc-link-lib=gtk-3");
    println!("cargo:rustc-link-lib=gobject-2.0");
    println!("cargo:rustc-link-lib=glib-2.0");
    println!("cargo:rustc-link-lib=crypto");
}
