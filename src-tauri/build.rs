fn main() {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ncnn_lib_dir = manifest_dir.join("ncnn").join("lib");
    println!("cargo:rustc-link-search=native={}", ncnn_lib_dir.display());
    println!("cargo:rustc-link-lib=static=ncnn");
    println!("cargo:rerun-if-changed=ncnn/lib/ncnn.lib");
    tauri_build::build()
}
