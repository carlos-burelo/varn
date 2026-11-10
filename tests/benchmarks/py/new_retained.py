class P:
    __slots__ = ("x",)

    def __init__(self, x):
        self.x = x


count = 200000
items = [P(i) for i in range(count)]
total = 0
for item in items:
    total += item.x
print("Count:", len(items))
print("Total:", total)
