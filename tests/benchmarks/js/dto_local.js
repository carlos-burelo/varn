class User {
    constructor(id, name, age, isActive, score) {
        this.id = id; this.name = name; this.age = age; this.isActive = isActive; this.score = score;
    }
}
class UserDTO {
    constructor(id, displayName, score) { this.id = id; this.displayName = displayName; this.score = score; }
}
const users = [];
for (let i = 0; i < 100000; i++) {
    users.push(new User(i, `User_${i}`, (i % 60) + 10, (i % 2) === 0, ((i % 100) * 1.5)));
}
const start = performance.now();
const activeAdults = [];
let totalScore = 0;
for (let i = 0; i < 100000; i++) {
    const u = users[i];
    if (u.isActive && u.age >= 18) {
        activeAdults.push(new UserDTO(u.id, u.name, u.score));
        totalScore = totalScore + u.score;
    }
}
const end = performance.now();
console.log('Count:', activeAdults.length);
console.log('Total Score:', totalScore);
console.log('Elapsed time (ms):', end - start);
