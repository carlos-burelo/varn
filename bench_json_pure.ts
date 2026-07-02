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
for (let k = 0; k < iterations; k++) {
    let parsed = JSON.parse(jsonStr) as any[];
    let s = JSON.stringify(parsed);
}

console.log("Pure JSON loop finished");
