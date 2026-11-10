function* pingpong() { let x = 0; while (true) { x++; yield x; } }
function* rangeGen(start, end) { for (let i = start; i < end; i++) yield i; }
function* fibGen() { let a = 0n, b = 1n; while (true) { yield a; const t = a + b; a = b; b = t; } }
function* nestedInner(n) { yield n * 10; yield n * 20; }
function* nestedOuter() { yield 1; const inner = nestedInner(5); yield inner.next().value + 100; yield inner.next().value + 100; }
function makeCounter(step) {
    const base = step * 1000;
    return function* () { let i = 0; while (true) { yield base + i; i++; } };
}
function* guarded() { yield 7; yield 8; }
const now = () => performance.now();

let t0 = now();
const pp = pingpong();
let last = pp.next().value;
for (let i = 1; i <= 100000; i++) last = pp.next().value;
console.log(`pingpong checksum=${last} ms=${now() - t0}`);

t0 = now();
let mapSum = 0, filtSum = 0;
for (const v of rangeGen(0, 100000)) { mapSum += v * 2; if (v % 2 === 0) filtSum += v; }
console.log(`mapfilter map_sum=${mapSum} filt_sum=${filtSum} ms=${now() - t0}`);

t0 = now();
let fibCarry = 0n;
const fg = fibGen();
for (let i = 0; i < 80; i++) fibCarry = fg.next().value;
console.log(`fibcarry checksum=${fibCarry} ms=${now() - t0}`);

t0 = now();
let nestSum = 0;
for (const item of nestedOuter()) nestSum += item;
console.log(`nested checksum=${nestSum} ms=${now() - t0}`);

t0 = now();
const counter = makeCounter(3);
const c1 = counter(), c2 = counter();
const capSum = c1.next().value + c1.next().value + c2.next().value;
console.log(`upvalue checksum=${capSum} ms=${now() - t0}`);

t0 = now();
let trySum = 0;
try { const gg = guarded(); trySum += gg.next().value; trySum += gg.next().value; } catch (e) { trySum = -1; }
console.log(`tryoutside checksum=${trySum} ms=${now() - t0}`);
