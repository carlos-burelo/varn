class Person {
  constructor(id, name, age, score) { this.id = id; this.name = name; this.age = age; this.score = score; }
}
const t0 = performance.now();
const people = [];
let seed = 12345;
for (let i = 0; i < 100000; i++) {
  seed = (seed * 1103515245 + 12345) % 2147483648;
  const age = 18 + (seed % 60);
  seed = (seed * 1103515245 + 12345) % 2147483648;
  const score = (seed % 10000) / 100.0;
  people.push(new Person(i, `name_${seed % 5000}`, age, score));
}
const byAge = people.slice().sort((a, b) => a.age - b.age);
const byName = people.slice().sort((a, b) => a.name < b.name ? -1 : (a.name > b.name ? 1 : 0));
const byScore = people.slice().sort((a, b) => b.score - a.score);
let check = 0;
for (let i = 0; i < 100000; i += 997) check += byAge[i].id + byName[i].id + byScore[i].id;
console.error(`sorted check=${check} first=${byAge[0].age} ${byName[0].name} ${byScore[0].score}`);
console.log((performance.now() - t0).toFixed(3));
