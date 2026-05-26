use varn_types::FunctionProto;

pub fn debug_cap_trace(proto: &FunctionProto, filename: &str) {
    use crate::colors::{BOLD, C_MODULES, DIM, R};
    println!("\n{BOLD}Capability Trace{R}  {DIM}{filename}{R}");
    if proto.required_caps.is_empty() {
        println!("  {DIM}(no capabilities required){R}");
    } else {
        println!("  Required capabilities:");
        for cap in &proto.required_caps {
            println!("    {C_MODULES}@cap{R}({BOLD}\"{cap}\"{R})");
        }
    }
}
