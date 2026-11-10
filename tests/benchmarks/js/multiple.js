function fib(n) { return n <= 1 ? n : fib(n - 1) + fib(n - 2); }
for (let r = 1; r <= 5; r++) {
    const start = performance.now();
    const result = fib(35);
    const elapsed = performance.now() - start;
    console.log('--- Run ---');
    console.log(r);
    console.log('Result:');
    console.log(result);
    console.log('Time (ms):');
    console.log(elapsed);
}
