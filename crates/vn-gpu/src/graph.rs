use crate::arena::Handle;

#[derive(Clone, Copy, PartialEq)]
pub enum Op {
    Add,
    Mul,
    Scale { factor: f32 },
    MatMul { m: usize, k: usize, n: usize },
}

pub(crate) fn run_op(
    op: Op,
    ins: &[Vec<f32>],
    _handles: &[Handle],
    _bufs: &crate::arena::Bufs,
    len: usize,
) -> Result<Vec<f32>, String> {
    match op {
        Op::Add | Op::Mul => {
            let (x, y) = (&ins[0], &ins[1]);
            if x.len() != y.len() || x.len() != len {
                return Err("gpu: length mismatch".to_string());
            }
            Ok(match op {
                Op::Add => x.iter().zip(y).map(|(a, b)| a + b).collect(),
                Op::Mul => x.iter().zip(y).map(|(a, b)| a * b).collect(),
                Op::Scale { .. } | Op::MatMul { .. } => {
                    return Err("gpu: length mismatch".to_string())
                }
            })
        }
        Op::Scale { factor } => {
            if ins.len() != 1 || ins[0].len() != len {
                return Err("gpu: scale shape".to_string());
            }
            Ok(ins[0].iter().map(|a| a * factor).collect())
        }
        Op::MatMul { m, k, n } => {
            let (x, y) = (&ins[0], &ins[1]);
            if x.len() != m * k || y.len() != k * n || len != m * n {
                return Err("gpu: matmul shape".to_string());
            }
            let mut out = vec![0.0f32; m * n];
            for i in 0..m {
                for j in 0..n {
                    let mut acc = 0.0f32;
                    for p in 0..k {
                        acc += x[i * k + p] * y[p * n + j];
                    }
                    out[i * n + j] = acc;
                }
            }
            Ok(out)
        }
    }
}
