use varn_op_macros::varn_contract;
use varn_types::{NativeCtx, Value, VmValue};

/// Native implementation backing the `runtime:json` contract
/// (`src/modules/std/json/runtime/json_runtime.vn`).
pub struct JsonRuntime;

varn_contract! {
    module: "runtime:json",
    contract: "src/modules/std/json/runtime/json_runtime.vn",
    impl JsonRuntime {
        fn jsonParse(ctx: &mut dyn NativeCtx, text: &str) -> VmValue {
            match serde_json::from_str::<serde_json::Value>(text) {
                Ok(sv) => serde_to_vm(ctx, sv),
                Err(_) => VmValue::null(),
            }
        }

        fn jsonStringify(ctx: &mut dyn NativeCtx, value: VmValue) -> String {
            let val = ctx.extract(value);
            let json_val = value_to_serde(&val, ctx);
            serde_json::to_string(&json_val).unwrap_or_else(|_| "null".to_string())
        }
    }
}

fn serde_to_vm(ctx: &mut dyn NativeCtx, sv: serde_json::Value) -> VmValue {
    match sv {
        serde_json::Value::Null => VmValue::null(),
        serde_json::Value::Bool(b) => VmValue::from_bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                VmValue::from_int(i)
            } else if let Some(f) = n.as_f64() {
                VmValue::from_f64(f)
            } else {
                VmValue::null()
            }
        }
        serde_json::Value::String(s) => ctx.alloc_str_owned(s),
        serde_json::Value::Array(arr) => {
            let items = arr.into_iter().map(|v| serde_to_vm(ctx, v)).collect();
            ctx.alloc_array(items)
        }
        serde_json::Value::Object(map) => {
            let obj = ctx.alloc_object();
            for (k, v) in map {
                let val = serde_to_vm(ctx, v);
                ctx.set_field(obj, &k, val);
            }
            obj
        }
    }
}

fn value_to_serde(val: &Value, ctx: &mut dyn NativeCtx) -> serde_json::Value {
    match val {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::Str(s) => serde_json::Value::String(s.to_string()),
        Value::Array(arr) => {
            let items = arr
                .0
                .borrow()
                .iter()
                .map(|v| value_to_serde(v, ctx))
                .collect();
            serde_json::Value::Array(items)
        }
        Value::Object(o) => {
            let guard = o.borrow();
            let map = guard
                .inner
                .iter()
                .map(|(k, nv)| {
                    let v = ctx.extract(nv);
                    (k.to_string(), value_to_serde(&v, ctx))
                })
                .collect();
            serde_json::Value::Object(map)
        }
        _ => serde_json::Value::Null,
    }
}
