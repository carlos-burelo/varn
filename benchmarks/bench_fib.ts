function fib(n: number): number {
    if (n <= 1) return n;
    return fib(n - 1) + fib(n - 2);
}

const start = performance.now();
const result = fib(35);
const elapsed = performance.now() - start;

console.log("fib(35) =");
console.log(result);
console.log("Elapsed time (ms):");
console.log(elapsed.toFixed(4));
