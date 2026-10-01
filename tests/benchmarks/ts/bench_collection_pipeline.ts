// Real-world Functional Collection Processing Pipeline in TypeScript

class OrderItem {
    id: number;
    sku: string;
    category: string;
    quantity: number;
    unitPrice: number;
    isDiscounted: boolean;
    status: string;

    constructor(id: number, sku: string, category: string, quantity: number, unitPrice: number, isDiscounted: boolean, status: string) {
        this.id = id;
        this.sku = sku;
        this.category = category;
        this.quantity = quantity;
        this.unitPrice = unitPrice;
        this.isDiscounted = isDiscounted;
        this.status = status;
    }
}

class OrderSummary {
    id: number;
    category: string;
    grossTotal: number;
    tax: number;
    netTotal: number;

    constructor(id: number, category: string, grossTotal: number, tax: number, netTotal: number) {
        this.id = id;
        this.category = category;
        this.grossTotal = grossTotal;
        this.tax = tax;
        this.netTotal = netTotal;
    }
}

// 1. Generate 100,000 Order Items
const categories = ["Electronics", "Apparel", "Home", "Books", "Sports"];
const statuses = ["completed", "pending", "cancelled", "completed", "completed"];

const orders: OrderItem[] = [];
for (let i = 0; i < 100000; i++) {
    const cat = categories[i % 5];
    const stat = statuses[i % 5];
    const qty = (i % 8) + 1;
    const price = ((i % 150) + 10) * 1.5;
    const disc = (i % 3) === 0;
    orders.push(new OrderItem(
        i,
        `SKU_${i % 2000}`,
        cat,
        qty,
        price,
        disc,
        stat
    ));
}

// 2. Multi-Stage Pipeline Execution

// Stage 1: Filter active completed orders
const validOrders: OrderItem[] = [];
for (let i = 0; i < orders.length; i++) {
    const o = orders[i];
    if (o.status === "completed" && o.quantity > 0) {
        validOrders.push(o);
    }
}

// Stage 2: Map to financial summaries with tax and discounts
const summaries: OrderSummary[] = [];
for (let i = 0; i < validOrders.length; i++) {
    const o = validOrders[i];
    let gross = o.quantity * o.unitPrice;
    if (o.isDiscounted) {
        gross = gross * 0.9;
    }
    const tax = gross * 0.16;
    const net = gross + tax;
    summaries.push(new OrderSummary(o.id, o.category, gross, tax, net));
}

// Stage 3: Filter high value summaries
const highValueSummaries: OrderSummary[] = [];
for (let i = 0; i < summaries.length; i++) {
    const s = summaries[i];
    if (s.netTotal >= 50.0) {
        highValueSummaries.push(s);
    }
}

// Stage 4: Reduce to totals
let totalGross = 0.0;
let totalTax = 0.0;
let totalNet = 0.0;

for (let i = 0; i < highValueSummaries.length; i++) {
    const s = highValueSummaries[i];
    totalGross += s.grossTotal;
    totalTax += s.tax;
    totalNet += s.netTotal;
}

// Stage 5: Find & Verification
let foundItem: OrderSummary | null = null;
const targetId = 4242;
for (let i = 0; i < highValueSummaries.length; i++) {
    if (highValueSummaries[i].id === targetId) {
        foundItem = highValueSummaries[i];
        break;
    }
}

console.log(`Collection Pipeline: Processed ${orders.length} orders -> ${highValueSummaries.length} high value summaries, Total Net = $${totalNet}, Target Found = ${foundItem !== null}`);
