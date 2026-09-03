from _harness import run


class GcVtA:
    __slots__ = ("x",)

    def __init__(self, x):
        self.x = x


class GcVtB:
    __slots__ = ("y",)

    def __init__(self, y):
        self.y = y


def compute():
    junk = []
    for i in range(400000):
        junk.append("gc_" + str(i))
    aa = 0
    bb = 0
    for i in range(100000):
        a = GcVtA(i)
        aa += a.x
        b = GcVtB(i)
        bb += b.y
    return f"check={aa}/{bb}/{len(junk)}"


run(compute)
