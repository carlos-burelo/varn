const { PerformanceObserver } = require('node:perf_hooks');
const gcs = [];
const obs = new PerformanceObserver(list => { for (const e of list.getEntries()) gcs.push({ d: e.duration, k: (e.detail && e.detail.kind) || e.kind }); });
obs.observe({ entryTypes: ['gc'] });
class Node { constructor(id) { this.id = id; } }
const W = process.argv[2];
function w1() { let t = 0; for (let i = 0; i < 300; i++) { const a = new Array(500000).fill(i); t += a.length; } return t; }
function w3() {
  const keep = []; for (let i = 0; i < 4000000; i++) keep.push(new Node(i));
  const ring = []; for (let i = 0; i < 1024; i++) ring.push(new Node(0));
  for (let i = 0; i < 4000000; i++) ring[i & 1023] = new Node(i);
  return keep.length + ring[5].id;
}
function w5() {
  const size = 500000, win = []; for (let i = 0; i < size; i++) win.push(new Node(i));
  let s = 0; for (let i = 0; i < 6000000; i++) { win[i % size] = new Node(i); s += win[(i + 7) % size].id; }
  return s;
}
const t0 = process.hrtime.bigint();
const r = { w1, w3, w5 }[W]();
const wall = Number(process.hrtime.bigint() - t0) / 1e6;
setTimeout(() => {
  for (const e of obs.takeRecords()) gcs.push({ d: e.duration, k: (e.detail && e.detail.kind) || e.kind });
  const major = gcs.filter(g => g.k === 4), minor = gcs.filter(g => g.k === 1);
  const sum = a => a.reduce((x, g) => x + g.d, 0), max = a => a.reduce((x, g) => Math.max(x, g.d), 0);
  console.log(JSON.stringify({ wall_ms: +wall.toFixed(0), gcs: gcs.length, pause_total_ms: +sum(gcs).toFixed(1), minor_max_ms: +max(minor).toFixed(1), major_max_ms: +max(major).toFixed(1), result: r }));
}, 100);
