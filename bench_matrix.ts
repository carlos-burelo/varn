const n = 150;
const size = n * n;

const a: number[] = [];
const b: number[] = [];
const c: number[] = [];

for (let i = 0; i < size; i++) {
    a.push((i % 100) + 1);
    b.push(((i * 3) % 100) + 1);
    c.push(0);
}

const start = performance.now();

for (let row = 0; row < n; row++) {
    for (let col = 0; col < n; col++) {
        let sum = 0;
        for (let k = 0; k < n; k++) {
            sum += a[row * n + k] * b[k * n + col];
        }
        c[row * n + col] = sum;
    }
}

const end = performance.now();

console.log("c[0] =", c[0]);
console.log("c[last] =", c[size - 1]);
console.log("Elapsed time (ms):", (end - start).toFixed(4));
