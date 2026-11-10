let sum = 0;
for (let i = 0; i < 10000000; i++) {
    const x = { a: i, b: i + 1, c: i + 2 };
    sum += x.a;
}
console.log(sum);
