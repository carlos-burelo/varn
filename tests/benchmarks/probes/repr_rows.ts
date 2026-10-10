class P3 { constructor(public a: number, public b: number, public c: number) {} }
const N = 1000000;
let checksum = 0;
function row(name: string, f: () => number) {
    let best = 1e9;
    for (let r = 0; r < 5; r++) {
        const t = performance.now();
        checksum += f();
        const d = performance.now() - t;
        if (d < best) best = d;
    }
    console.log(name + " " + Math.round(best));
}
row("loop_only", () => { const k: number[] = []; for (let i = 0; i < N; i++) k.push(i); return k.length; });
row("instance3", () => { const k: P3[] = []; for (let i = 0; i < N; i++) k.push(new P3(i, 2, 3)); return k.length; });
row("objlit3", () => { const k: any[] = []; for (let i = 0; i < N; i++) k.push({ a: i, b: 2, c: 3 }); return k.length; });
row("arraylit3", () => { const k: number[][] = []; for (let i = 0; i < N; i++) k.push([i, 2, 3]); return k.length; });
row("map3", () => { const k: Record<string, number>[] = []; for (let i = 0; i < N; i++) k.push({ "a": i, "b": 2, "c": 3 }); return k.length; });
row("map_get_heapkey", () => {
    const m: Record<string, string> = { "authorization": "Bearer x", "content-type": "json", "user-agent": "probe" };
    const keys = ["authorization", "content-type", "user-agent"];
    let total = 0;
    for (let i = 0; i < N * 2; i++) total += m[keys[i % 3]].length;
    return total;
});
row("strview", () => { const k: string[] = []; const base = "abcdefghijklmnopqrstuvwxyz0123456789"; for (let i = 0; i < N; i++) k.push(base.slice(i % 10, 20)); return k.length; });
row("split5", () => { const k: string[][] = []; for (let i = 0; i < N / 2; i++) k.push("/api/v1/orders/history/details".split("/")); return k.length; });
console.log("checksum " + checksum);
