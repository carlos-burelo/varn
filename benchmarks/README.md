# Benchmarks

Standalone `.vn` micro-benchmarks and their JavaScript counterparts
(`.ts`/`.js`) for paired comparison against Node.

Run one with the CLI's timing harness:

```
vn bench benchmarks/bench_fib.vn
```

Or run and compare against the JS baseline manually:

```
vn run benchmarks/bench_fib.vn
node benchmarks/bench_fib.ts   # if a paired baseline exists
```

## Measurement discipline

The machine throttles ~2x under sustained load (a `cargo build` heats it),
so only same-moment paired runs are comparable. Take a cold reading first
as a thermal canary before trusting absolute numbers, and always run the
Varn and Node sides back-to-back.

The broad correctness + timing suite lives in `tests/main.vn`
(`vn bench tests/main.vn`), not here.
