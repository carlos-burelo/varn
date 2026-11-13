const t0 = performance.now();
const words = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu"];
const parts = [];
let seed = 7;
for (let i = 0; i < 300000; i++) {
  seed = (seed * 1103515245 + 12345) % 2147483648;
  parts.push(words[seed % 12] + (seed % 50).toString());
}
const text = parts.join(" ");
const counts = new Map();
const tokens = text.split(" ");
for (let i = 0; i < tokens.length; i++) {
  const t = tokens[i];
  const c = counts.get(t);
  counts.set(t, (c ?? 0) + 1);
}
let best = "", bestCount = 0, total = 0;
for (const [word, n] of counts) {
  total += n;
  if (n > bestCount || (n === bestCount && word < best)) { best = word; bestCount = n; }
}
console.error(`words=${tokens.length} distinct=${counts.size} best=${best}:${bestCount} total=${total}`);
console.log((performance.now() - t0).toFixed(3));
