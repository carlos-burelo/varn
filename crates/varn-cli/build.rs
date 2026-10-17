const DELAY_LOADED_DLLS: &[&str] = &[
    "opengl32.dll",
    "GLU32.dll",
    "d3dcompiler_47.dll",
    "dxcore.dll",
    "dwmapi.dll",
    "uxtheme.dll",
    "POWRPROF.dll",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        for dll in DELAY_LOADED_DLLS {
            println!("cargo:rustc-link-arg-bin=vn=/DELAYLOAD:{dll}");
        }
        println!("cargo:rustc-link-arg-bin=vn=delayimp.lib");
    }
}
