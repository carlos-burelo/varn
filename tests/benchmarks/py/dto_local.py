import time


class User:
    __slots__ = ("id", "name", "age", "is_active", "score")

    def __init__(self, id, name, age, is_active, score):
        self.id = id
        self.name = name
        self.age = age
        self.is_active = is_active
        self.score = score


class UserDTO:
    __slots__ = ("id", "display_name", "score")

    def __init__(self, id, display_name, score):
        self.id = id
        self.display_name = display_name
        self.score = score


count = 100000
users = [User(i, f"User_{i}", (i % 60) + 10, (i % 2) == 0, (i % 100) * 1.5) for i in range(count)]

start = time.perf_counter()
active = []
total_score = 0.0
for u in users:
    if u.is_active and u.age >= 18:
        active.append(UserDTO(u.id, u.name, u.score))
        total_score += u.score
end = time.perf_counter()

print("Count:", len(active))
print("Total Score:", total_score)
print("Elapsed time (ms):", (end - start) * 1000.0)
