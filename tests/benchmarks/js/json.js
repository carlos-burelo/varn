const items = [];
for (let i = 0; i < 50000; i++) {
    items.push({ id: i, name: `Item_${i}`, active: (i % 2) === 0, score: (i % 100) * 1.25 });
}
const jsonStr = JSON.stringify(items);
const parsed = JSON.parse(jsonStr);
console.log(parsed.length);
let totalScore = 0;
for (let i = 0; i < parsed.length; i++) totalScore += parsed[i].score;
console.log(Math.trunc(totalScore).toString());
console.log('bytes: ' + jsonStr.length);
