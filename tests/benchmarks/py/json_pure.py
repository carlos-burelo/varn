import json

items = []
for i in range(50000):
    items.append({"id": i, "name": f"Item_{i}", "active": (i % 2) == 0, "score": (i % 100) * 1.25})
text = json.dumps(items)
parsed = json.loads(text)
for _ in range(20):
    parsed = json.loads(text)
    json.dumps(parsed)
print(len(parsed))
print(str(sum(item["id"] for item in parsed)))
