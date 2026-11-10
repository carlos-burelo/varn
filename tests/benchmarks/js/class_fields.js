class Point {
    constructor(x, y) { this.x = x; this.y = y; }
}
const pt = new Point(10, 20);
let sum = 0;
for (let i = 0; i < 1000000; i++) {
    pt.x = pt.x + 1;
    pt.y = pt.y + 2;
    sum = sum + pt.x + pt.y;
}
console.log('Class field sum: ' + sum);
