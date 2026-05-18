use varn_core::OpCode;

fn main() {
    println!("OpPushConst: {}", OpCode::OpPushConst as u16);
    println!("OpPop: {}", OpCode::OpPop as u16);
    println!("OpClosure: {}", OpCode::OpClosure as u16);
    println!("OpCall: {}", OpCode::OpCall as u16);
    println!("OpRegAdd: {}", OpCode::OpRegAdd as u16);
    println!("OpRegMove: {}", OpCode::OpRegMove as u16);
}
