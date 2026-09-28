fn main() {
    println!("cargo:rerun-if-changed=../varn-vm/src/exec/jit_helpers");
}
