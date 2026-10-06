class P {
    x: number;

    constructor(x: number) {
        this.x = x;
    }
}

function run(): void {
    const count = 200000;

    const items: P[] = [];
    for (let i = 0; i < count; i++) {
        items.push(new P(i));
    }

    let total = 0;
    for (let i = 0; i < count; i++) {
        total = total + items[i].x;
    }

    console.log("Count:", items.length);
    console.log("Total:", total);
}

run();
