class P { constructor(x) { this.x = x; } }
const count = 200000;
const items = [];
for (let i = 0; i < count; i++) items.push(new P(i));
let total = 0;
for (let i = 0; i < count; i++) total += items[i].x;
console.log('Count:', items.length);
console.log('Total:', total);
