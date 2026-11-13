class Circle { constructor(r) { this.r = r; } area() { return 3.14159 * this.r * this.r; } scale(f) { return new Circle(this.r * f); } }
class Rect { constructor(w, h) { this.w = w; this.h = h; } area() { return this.w * this.h; } scale(f) { return new Rect(this.w * f, this.h * f); } }
class Square { constructor(s) { this.s = s; } area() { return this.s * this.s; } scale(f) { return new Square(this.s * f); } }
const t0 = performance.now();
const shapes = [];
for (let i = 0; i < 300000; i++) {
  const k = i % 3;
  if (k === 0) shapes.push(new Circle((i % 10) + 1.0));
  else if (k === 1) shapes.push(new Rect((i % 7) + 1.0, (i % 5) + 1.0));
  else shapes.push(new Square((i % 9) + 1.0));
}
let total = 0.0;
for (let round = 0; round < 10; round++) for (let i = 0; i < shapes.length; i++) total += shapes[i].scale(1.01).area();
console.error(`polymorphism total=${total}`);
console.log((performance.now() - t0).toFixed(3));
