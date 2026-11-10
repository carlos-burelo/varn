class Point:
    __slots__ = ("x", "y")

    def __init__(self, x, y):
        self.x = x
        self.y = y


pt = Point(10, 20)
total = 0
for _ in range(1000000):
    pt.x = pt.x + 1
    pt.y = pt.y + 2
    total = total + pt.x + pt.y
print("Class field sum: " + str(total))
