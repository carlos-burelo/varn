const items = [];
for (let i = 0; i < 50000; i++) {
    items.push({ id: i, name: `Item_${i}`, active: (i % 2) === 0, score: (i % 100) * 1.25 });
}
const jsonStr = JSON.stringify(items);
let parsed = JSON.parse(jsonStr);
for (let k = 0; k < 20; k++) {
    parsed = JSON.parse(jsonStr);
    JSON.stringify(parsed);
}
console.log(parsed.length);
let idSum = 0;
for (let i = 0; i < parsed.length; i++) idSum += parsed[i].id;
console.log(idSum.toString());
