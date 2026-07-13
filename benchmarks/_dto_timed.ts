class User {
    id: number; name: string; age: number; isActive: boolean; score: number;
    constructor(id: number, name: string, age: number, isActive: boolean, score: number) {
        this.id = id; this.name = name; this.age = age; this.isActive = isActive; this.score = score;
    }
}
class UserDTO {
    id: number; displayName: string; score: number;
    constructor(id: number, displayName: string, score: number) {
        this.id = id; this.displayName = displayName; this.score = score;
    }
}
const count = 100000;
const t0 = performance.now();
const users: User[] = [];
for (let i = 0; i < count; i++) {
    users.push(new User(i, "User_" + i, (i % 60) + 10, (i % 2) === 0, (i % 100) * 1.5));
}
const t1 = performance.now();
const activeAdults: UserDTO[] = [];
let totalScore = 0.0;
for (let i = 0; i < count; i++) {
    const u = users[i];
    if (u.isActive && u.age >= 18) {
        const dto = new UserDTO(u.id, u.name, u.score);
        activeAdults.push(dto);
        totalScore += u.score;
    }
}
const t2 = performance.now();
console.log("build_ms=" + (t1 - t0).toFixed(2));
console.log("transform_ms=" + (t2 - t1).toFixed(2));
console.log("check=" + activeAdults.length + "/" + totalScore);
