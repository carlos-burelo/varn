fn main() {
    println!("cargo:rerun-if-changed=../vn-vm/src/exec/jit_helpers");
}
