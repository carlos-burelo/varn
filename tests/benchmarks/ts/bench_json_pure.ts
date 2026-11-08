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

let iterations = 20;
let parsed = JSON.parse(jsonStr) as any[];
for (let k = 0; k < iterations; k++) {
    parsed = JSON.parse(jsonStr) as any[];
    let s = JSON.stringify(parsed);
}

console.log(parsed.length);
let idSum = 0;
for (let i = 0; i < parsed.length; i++) {
    idSum += parsed[i]["id"];
}
console.log(idSum);
