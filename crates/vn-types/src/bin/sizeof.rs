#![allow(unused_crate_dependencies)]

use vn_core::ast::expr::Expr;
use vn_core::ast::stmt::Stmt;
use vn_types::Value;

fn main() {
    println!("Size of Value: {} bytes", std::mem::size_of::<Value>());
    println!("Size of Expr: {} bytes", std::mem::size_of::<Expr>());
    println!("Size of Stmt: {} bytes", std::mem::size_of::<Stmt>());
    println!(
        "Size of Option<Value>: {} bytes",
        std::mem::size_of::<Option<Value>>()
    );
}
