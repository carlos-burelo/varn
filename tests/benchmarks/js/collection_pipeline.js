class OrderItem {
    constructor(id, sku, category, quantity, unitPrice, isDiscounted, status) {
        this.id = id; this.sku = sku; this.category = category; this.quantity = quantity;
        this.unitPrice = unitPrice; this.isDiscounted = isDiscounted; this.status = status;
    }
}
class OrderSummary {
    constructor(id, category, grossTotal, tax, netTotal) {
        this.id = id; this.category = category; this.grossTotal = grossTotal; this.tax = tax; this.netTotal = netTotal;
    }
}
const categories = ['Electronics', 'Apparel', 'Home', 'Books', 'Sports'];
const statuses = ['completed', 'pending', 'cancelled', 'completed', 'completed'];
const orders = [];
for (let i = 0; i < 100000; i++) {
    orders.push(new OrderItem(i, `SKU_${i % 2000}`, categories[i % 5], (i % 8) + 1, ((i % 150) + 10) * 1.5, (i % 3) === 0, statuses[i % 5]));
}
const validOrders = [];
for (let i = 0; i < orders.length; i++) {
    const o = orders[i];
    if (o.status === 'completed' && o.quantity > 0) validOrders.push(o);
}
const summaries = [];
for (let i = 0; i < validOrders.length; i++) {
    const o = validOrders[i];
    let gross = o.quantity * o.unitPrice;
    if (o.isDiscounted) gross = gross * 0.9;
    const tax = gross * 0.16;
    summaries.push(new OrderSummary(o.id, o.category, gross, tax, gross + tax));
}
const high = [];
for (let i = 0; i < summaries.length; i++) if (summaries[i].netTotal >= 50.0) high.push(summaries[i]);
let totalNet = 0;
for (let i = 0; i < high.length; i++) totalNet = totalNet + high[i].netTotal;
let found = false;
for (let i = 0; i < high.length; i++) if (high[i].id === 4242) { found = true; break; }
console.log(`Collection Pipeline: Processed ${orders.length} orders -> ${high.length} high value summaries, Total Net = $${totalNet}, Target Found = ${found}`);
