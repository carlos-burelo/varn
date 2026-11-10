class OrderItem:
    __slots__ = ("id", "sku", "category", "quantity", "unit_price", "is_discounted", "status")

    def __init__(self, id, sku, category, quantity, unit_price, is_discounted, status):
        self.id = id
        self.sku = sku
        self.category = category
        self.quantity = quantity
        self.unit_price = unit_price
        self.is_discounted = is_discounted
        self.status = status


class OrderSummary:
    __slots__ = ("id", "category", "gross_total", "tax", "net_total")

    def __init__(self, id, category, gross_total, tax, net_total):
        self.id = id
        self.category = category
        self.gross_total = gross_total
        self.tax = tax
        self.net_total = net_total


categories = ["Electronics", "Apparel", "Home", "Books", "Sports"]
statuses = ["completed", "pending", "cancelled", "completed", "completed"]
orders = [
    OrderItem(i, f"SKU_{i % 2000}", categories[i % 5], (i % 8) + 1, ((i % 150) + 10) * 1.5,
              (i % 3) == 0, statuses[i % 5])
    for i in range(100000)
]
valid = [o for o in orders if o.status == "completed" and o.quantity > 0]
summaries = []
for o in valid:
    gross = o.quantity * o.unit_price
    if o.is_discounted:
        gross = gross * 0.9
    tax = gross * 0.16
    summaries.append(OrderSummary(o.id, o.category, gross, tax, gross + tax))
high = [s for s in summaries if s.net_total >= 50.0]
total_net = 0.0
for s in high:
    total_net += s.net_total
found = any(s.id == 4242 for s in high)
print(f"Collection Pipeline: Processed {len(orders)} orders -> {len(high)} high value summaries, "
      f"Total Net = ${total_net}, Target Found = {found}")
