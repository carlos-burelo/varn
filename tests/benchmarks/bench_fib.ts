// Prints exactly what bench_fib.vn prints, and nothing else.
//
// This used to add a label and an internal `performance.now()` timing. Both
// made the row incomparable: the harness compares computed output across
// runtimes to catch a benchmark that is fast because it is wrong, and the
// label's "35" plus the timing made every comparison a mismatch. The extra
// console.log calls were also work the .vn port never did.
function fib(n: number): number {
    if (n <= 1) return n;
    return fib(n - 1) + fib(n - 2);
}

console.log(fib(35));
