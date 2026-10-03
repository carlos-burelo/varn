#[derive(Debug, Clone)]
pub(super) struct BenchDef {
    pub(super) name: &'static str,
    pub(super) vn: &'static str,
    pub(super) ts: &'static str,
    pub(super) py: Option<&'static str>,
}

pub(super) const ALL_BENCHMARKS: &[BenchDef] = &[
    BenchDef {
        name: "fib",
        vn: "bench_fib.vn",
        ts: "bench_fib.ts",
        py: Some("py/fib.py"),
    },
    BenchDef {
        name: "gc_alloc",
        vn: "bench_gc_alloc.vn",
        ts: "bench_gc_alloc.ts",
        py: Some("py/gc_alloc.py"),
    },
    BenchDef {
        name: "dto",
        vn: "bench_dto_local.vn",
        ts: "bench_dto.ts",
        py: Some("py/dto.py"),
    },
    BenchDef {
        name: "matrix",
        vn: "bench_matrix.vn",
        ts: "bench_matrix.ts",
        py: Some("py/matrix.py"),
    },
    BenchDef {
        name: "str_ops",
        vn: "bench_str_ops.vn",
        ts: "bench_str_ops.ts",
        py: None,
    },
    BenchDef {
        name: "json_native",
        vn: "bench_json.vn",
        ts: "bench_json.ts",
        py: None,
    },
    BenchDef {
        name: "json_pure",
        vn: "bench_json_pure.vn",
        ts: "bench_json_pure.ts",
        py: None,
    },
    BenchDef {
        name: "csv_pipeline",
        vn: "bench_csv_pipeline.vn",
        ts: "bench_csv_pipeline.ts",
        py: None,
    },
    BenchDef {
        name: "collection_pipeline",
        vn: "bench_collection_pipeline.vn",
        ts: "bench_collection_pipeline.ts",
        py: None,
    },
    BenchDef {
        name: "http_routing",
        vn: "bench_http_routing.vn",
        ts: "bench_http_routing.ts",
        py: None,
    },
    BenchDef {
        name: "csv_etl",
        vn: "bench_csv_etl.vn",
        ts: "bench_csv_etl.ts",
        py: None,
    },
    BenchDef {
        name: "json_api_payloads",
        vn: "bench_json_api_payloads.vn",
        ts: "bench_json_api_payloads.ts",
        py: None,
    },
];

pub(super) fn select_benchmarks(
    only: &Option<Vec<String>>,
) -> Result<Vec<&'static BenchDef>, String> {
    let benchmarks: Vec<&BenchDef> = if let Some(ref only_list) = only {
        let filtered: Vec<&BenchDef> = ALL_BENCHMARKS
            .iter()
            .filter(|b| only_list.iter().any(|o| o.eq_ignore_ascii_case(b.name)))
            .collect();
        if filtered.is_empty() {
            eprintln!("error: no benchmark matched: {}", only_list.join(", "));
            std::process::exit(1);
        }
        filtered
    } else {
        ALL_BENCHMARKS.iter().collect()
    };
    Ok(benchmarks)
}
