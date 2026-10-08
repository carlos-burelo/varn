fn main() {
    println!("cargo:rerun-if-changed=../../std");
    println!("cargo:rerun-if-changed=../../crates/varn-builtins");

    let std_dir = std::path::Path::new("../../std");

    std::env::set_var(varn_modules::std_root::ENV_VARN_STD, std_dir);
    varn_builtins::register_provider();
    if let Some(reason) = varn_builtins::std_load_error() {
        panic!("cannot build the stdlib bundle: {reason}");
    }

    let session = varn_pipeline::resolver::Session::new();
    let bytes = varn_pipeline::stdlib_loader::compile_stdlib_bundle(std_dir, &session)
        .expect("failed to compile stdlib bundle");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest_path = std::path::Path::new(&out_dir).join("std.vnb");
    std::fs::write(dest_path, bytes).expect("failed to write compiled stdlib bundle");
}
