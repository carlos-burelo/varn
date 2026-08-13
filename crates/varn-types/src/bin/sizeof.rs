#![allow(unused_crate_dependencies)] // dev-tool bin: uses a slice of the crate deps

use varn_core::ast::expr::Expr;
use varn_core::ast::stmt::Stmt;
use varn_types::Value;

fn main() {
    println!("Size of Value: {} bytes", std::mem::size_of::<Value>());
    println!("Size of Expr: {} bytes", std::mem::size_of::<Expr>());
    println!("Size of Stmt: {} bytes", std::mem::size_of::<Stmt>());
    println!(
        "Size of Option<Value>: {} bytes",
        std::mem::size_of::<Option<Value>>()
    );
}
