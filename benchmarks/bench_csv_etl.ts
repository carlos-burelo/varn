// Real-world CSV ETL: Parse, Validate, Enrich, Aggregate, and Re-export in TypeScript

// 1. Generate realistic 10,000 row CSV dataset
const csvLines: string[] = ["id,customer_id,product_category,amount,tax_rate,status,created_at"];
const categories = ["Electronics", "Fashion", "Home", "Automotive", "Garden"];
const statuses = ["completed", "refunded", "completed", "pending", "completed"];

for (let i = 1; i <= 10000; i++) {
    const cat = categories[i % 5];
    const stat = statuses[i % 5];
    const amt = ((i * 17) % 1000) + 15;
    const tax = 0.16;
    const cust = "CUST_" + (i % 800);
    const date = "2026-08-" + ((i % 28) + 1);
    csvLines.push(`${i},${cust},${cat},${amt},${tax},${stat},${date}`);
}

const rawCsv = csvLines.join("\n");

// 2. Parse CSV Dataset
function parseCsv(text: string): Record<string, any>[] {
    const lines = text.trim().split("\n");
    if (lines.length === 0) return [];
    const headers = lines[0].split(",");
    const rows: Record<string, any>[] = [];
    for (let i = 1; i < lines.length; i++) {
        const line = lines[i].trim();
        if (line.length === 0) continue;
        const vals = line.split(",");
        const row: Record<string, any> = {};
        for (let h = 0; h < headers.length; h++) {
            const val = vals[h];
            const num = Number(val);
            row[headers[h]] = !isNaN(num) && val !== "" ? num : val;
        }
        rows.push(row);
    }
    return rows;
}

const parsedRows = parseCsv(rawCsv);

// 3. Filter & Enrich Records
const enrichedRows: Record<string, any>[] = [];
let totalCompletedRevenue = 0.0;
let vipCount = 0;

for (let i = 0; i < parsedRows.length; i++) {
    const row = parsedRows[i];
    if (row["status"] === "completed") {
        const amt = Number(row["amount"]);
        const taxRate = Number(row["tax_rate"]);
        const finalTotal = amt * (1.0 + taxRate);
        const isVip = amt >= 500.0;

        if (isVip) {
            vipCount++;
        }
        totalCompletedRevenue += finalTotal;

        enrichedRows.push({
            "id": row["id"],
            "customer_id": row["customer_id"],
            "category": row["product_category"],
            "base_amount": amt,
            "final_total": finalTotal,
            "tier": isVip ? "VIP" : "Standard",
            "date": row["created_at"]
        });
    }
}

// 4. Serialize back to CSV format
function stringifyCsv(rows: Record<string, any>[]): string {
    if (rows.length === 0) return "";
    const headers = Object.keys(rows[0]);
    const out: string[] = [headers.join(",")];
    for (let i = 0; i < rows.length; i++) {
        const row = rows[i];
        const vals = headers.map(h => String(row[h]));
        out.push(vals.join(","));
    }
    return out.join("\n");
}

const exportedCsv = stringifyCsv(enrichedRows);

console.log("CSV ETL Benchmark:");
console.log("  Total parsed rows: " + parsedRows.length);
console.log("  Enriched completed rows: " + enrichedRows.length);
console.log("  VIP Transactions: " + vipCount);
console.log("  Total Completed Revenue: " + totalCompletedRevenue);
console.log("  Exported CSV length: " + exportedCsv.length + " bytes");
