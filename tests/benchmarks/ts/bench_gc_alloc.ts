// GC allocation-pressure benchmark — JS twin of bench_gc_alloc.vn.
class GcVtA {
    x: number;
    constructor(x: number) {
        this.x = x;
    }
}

class GcVtB {
    y: number;
    constructor(y: number) {
        this.y = y;
    }
}

const t0 = performance.now();

const gcJunk: string[] = [];
for (let i = 0; i < 400000; i = i + 1) {
    gcJunk.push("gc_" + i);
}

const t1 = performance.now();

let gcAccA = 0;
let gcAccB = 0;
for (let i = 0; i < 100000; i = i + 1) {
    const a = new GcVtA(i);
    gcAccA = gcAccA + a.x;
    const b = new GcVtB(i);
    gcAccB = gcAccB + b.y;
}

const t2 = performance.now();

console.log("junk_ms=" + (t1 - t0).toFixed(4));
console.log("alloc_ms=" + (t2 - t1).toFixed(4));
console.log("check=" + gcAccA + "/" + gcAccB + "/" + gcJunk.length);
