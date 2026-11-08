let items: any[] = [];
for (let i = 0; i < 50000; i++) {
    items.push({
        id: i,
        name: "Item_" + i,
        active: (i % 2) === 0,
        score: (i % 100) * 1.25
    });
}

let jsonStr = JSON.stringify(items);
let parsed = JSON.parse(jsonStr) as any[];

console.log(parsed.length);
let totalScore = 0;
for (let i = 0; i < parsed.length; i++) {
    totalScore += parsed[i]["score"];
}
console.log(Math.trunc(totalScore));
console.log("bytes: " + jsonStr.length);
