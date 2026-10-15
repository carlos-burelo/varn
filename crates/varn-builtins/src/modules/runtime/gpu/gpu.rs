use varn_macros::varn_contract;
use varn_types::marshal::VnArray;
use varn_types::{NativeCtx, VmValue};

pub struct GpuRuntime;

fn read_host(ctx: &dyn NativeCtx, arr: VnArray) -> Result<Vec<f32>, String> {
    let len = arr.len(ctx);
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let v = arr
            .get(ctx, i)
            .ok_or_else(|| "gpu.upload: index out of bounds".to_string())?;
        if !(v.is_f64() || v.is_int()) {
            return Err(format!("gpu.upload: expected float[], got {v:?}"));
        }
        out.push(v.to_f64() as f32);
    }
    Ok(out)
}

fn read_dims(ctx: &dyn NativeCtx, arr: VnArray) -> Result<Vec<usize>, String> {
    let len = arr.len(ctx);
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let v = arr
            .get(ctx, i)
            .ok_or_else(|| "gpu.reshape: index out of bounds".to_string())?;
        if !v.is_int() || v.as_int() < 0 {
            return Err(format!("gpu.reshape: expected int[] >= 0, got {v:?}"));
        }
        out.push(v.as_int() as usize);
    }
    Ok(out)
}

fn elementwise(op: varn_gpu::Op, a: i64, b: i64) -> Result<i64, String> {
    let pool = varn_gpu::pool();
    let g = pool.lock();
    let (la, lb) = (g.len(a)?, g.len(b)?);
    let (da, db) = (g.dims(a)?, g.dims(b)?);
    if la != lb || da != db {
        return Err(format!("gpu: shape mismatch ({da:?} vs {db:?})"));
    }
    drop(g);
    varn_gpu::pool().lock().lazy(op, vec![a, b], la, da)
}

varn_contract! {
    module: "runtime:gpu",
    contract: "src/modules/runtime/gpu/gpu_runtime.vn",
    impl GpuRuntime {
        fn backend(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(varn_gpu::backend_name().to_string())
        }
        fn deviceCount(_ctx: &mut dyn NativeCtx) -> Result<i64, String> {
            Ok(varn_gpu::device_count())
        }
        fn adapterName(_ctx: &mut dyn NativeCtx, i: i64) -> Result<String, String> {
            if i < 0 {
                return Err("gpu.adapterName: index must be non-negative".to_string());
            }
            varn_gpu::adapter_name(i as usize)
                .map(|s| s.to_string())
                .ok_or_else(|| format!("gpu.adapterName: no adapter at index {i}"))
        }
        fn engine(_ctx: &mut dyn NativeCtx) -> Result<String, String> {
            Ok(varn_gpu::last_engine().to_string())
        }
        fn create(_ctx: &mut dyn NativeCtx, len: i64) -> Result<i64, String> {
            if len < 0 {
                return Err("gpu.create: len must be non-negative".to_string());
            }
            Ok(varn_gpu::pool().lock().create(len as usize))
        }
        fn destroy(_ctx: &mut dyn NativeCtx, id: i64) -> Result<(), String> {
            varn_gpu::pool().lock().destroy(id)
        }
        fn len(_ctx: &mut dyn NativeCtx, id: i64) -> Result<i64, String> {
            Ok(varn_gpu::pool().lock().len(id)? as i64)
        }
        fn dims(ctx: &mut dyn NativeCtx, id: i64) -> Result<Vec<VmValue>, String> {
            let _ = ctx;
            Ok(varn_gpu::pool().lock().dims(id)?
                .into_iter()
                .map(|d| VmValue::from_int(d as i64))
                .collect())
        }
        fn reshape(ctx: &mut dyn NativeCtx, id: i64, dims: VnArray) -> Result<(), String> {
            let dims = read_dims(ctx, dims)?;
            varn_gpu::pool().lock().reshape(id, dims)
        }
        fn upload(ctx: &mut dyn NativeCtx, data: VnArray) -> Result<i64, String> {
            let vals = read_host(ctx, data)?;
            Ok(varn_gpu::pool().lock().upload(vals))
        }
        fn download(ctx: &mut dyn NativeCtx, id: i64) -> Result<Vec<VmValue>, String> {
            let _ = ctx;
            let vals = varn_gpu::pool().lock().data(id)?;
            Ok(vals.into_iter().map(|x| VmValue::from_f64(x as f64)).collect())
        }
        fn add(_ctx: &mut dyn NativeCtx, a: i64, b: i64) -> Result<i64, String> {
            elementwise(varn_gpu::Op::Add, a, b)
        }
        fn mul(_ctx: &mut dyn NativeCtx, a: i64, b: i64) -> Result<i64, String> {
            elementwise(varn_gpu::Op::Mul, a, b)
        }
        fn scale(_ctx: &mut dyn NativeCtx, a: i64, s: f64) -> Result<i64, String> {
            let pool = varn_gpu::pool();
            let (len, dims) = {
                let g = pool.lock();
                (g.len(a)?, g.dims(a)?)
            };
            varn_gpu::pool().lock().lazy(
                varn_gpu::Op::Scale { factor: s as f32 },
                vec![a],
                len,
                dims,
            )
        }
        fn dot(_ctx: &mut dyn NativeCtx, a: i64, b: i64) -> Result<f64, String> {
            let m = elementwise(varn_gpu::Op::Mul, a, b)?;
            let vals = varn_gpu::pool().lock().data(m)?;
            let acc: f64 = vals.iter().map(|x| *x as f64).sum();
            varn_gpu::pool().lock().destroy(m).ok();
            Ok(acc)
        }
        fn matMul(_ctx: &mut dyn NativeCtx, a: i64, b: i64) -> Result<i64, String> {
            let pool = varn_gpu::pool();
            let (da, db) = {
                let g = pool.lock();
                (g.dims(a)?, g.dims(b)?)
            };
            if da.len() != 2 || db.len() != 2 || da[1] != db[0] {
                return Err(format!("gpu.matMul: shape mismatch ({da:?} vs {db:?})"));
            }
            let (m, k, n) = (da[0], da[1], db[1]);
            varn_gpu::pool().lock().lazy(
                varn_gpu::Op::MatMul { m, k, n },
                vec![a, b],
                m * n,
                vec![m, n],
            )
        }
    }
}
