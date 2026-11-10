const items = [];
for (let i = 0; i < 50000; i++) {
    items.push({ id: i, name: `Item_${i}`, active: (i % 2) === 0, score: (i % 100) * 1.25 });
}
const jsonStr = JSON.stringify(items);
console.log(`Stringify done, len: ${jsonStr.length}`);
const parsed = JSON.parse(jsonStr);
console.log(`Parse done, len: ${parsed.length}`);
