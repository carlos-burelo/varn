from _harness import run


class User:
    __slots__ = ("id", "name", "age", "isActive", "score")

    def __init__(self, id, name, age, isActive, score):
        self.id = id
        self.name = name
        self.age = age
        self.isActive = isActive
        self.score = score


class UserDTO:
    __slots__ = ("id", "displayName", "score")

    def __init__(self, id, displayName, score):
        self.id = id
        self.displayName = displayName
        self.score = score


def compute():
    count = 100000
    users = []
    for i in range(count):
        users.append(User(i, "User_" + str(i), (i % 60) + 10, (i % 2) == 0, (i % 100) * 1.5))
    aa = []
    ts = 0.0
    for i in range(count):
        u = users[i]
        if u.isActive and u.age >= 18:
            aa.append(UserDTO(u.id, u.name, u.score))
            ts = ts + u.score
    return len(aa) + ts


run(compute)
