// Real-world CSV Ingestion, Filtering, Aggregation and Serialization Pipeline

function parseCSV(text: string): any[] {
    const lines = text.split("\n");
    if (lines.length === 0) return [];
    
    const headerLine = lines[0];
    if (!headerLine) return [];
    const headers = headerLine.split(",");
    
    const out: any[] = [];
    for (let i = 1; i < lines.length; i++) {
        const line = lines[i];
        if (!line) continue;
        const cols = line.split(",");
        const obj: any = {};
        for (let j = 0; j < headers.length; j++) {
            const h = headers[j];
            const val = cols[j];
            if (val === undefined) {
                obj[h] = null;
            } else if (val === "true") {
                obj[h] = true;
            } else if (val === "false") {
                obj[h] = false;
            } else {
                const num = Number(val);
                if (!isNaN(num) && val.trim() !== "") {
                    obj[h] = num;
                } else {
                    obj[h] = val;
                }
            }
        }
        out.push(obj);
    }
    return out;
}

function stringifyCSV(rows: any[]): string {
    if (rows.length === 0) return "";
    const headers = Object.keys(rows[0]);
    const lines: string[] = [headers.join(",")];
    
    for (let i = 0; i < rows.length; i++) {
        const row = rows[i];
        const cells: string[] = [];
        for (let j = 0; j < headers.length; j++) {
            const h = headers[j];
            const v = row[h];
            cells.push(v === null || v === undefined ? "" : String(v));
        }
        lines.push(cells.join(","));
    }
    return lines.join("\n");
}

// 1. Generate 50,000 transaction records in CSV format
const categories = ["Electronics", "Apparel", "Home", "Books", "Sports"];
const statuses = ["COMPLETED", "PENDING", "CANCELLED", "COMPLETED", "COMPLETED"];

const rows: string[] = ["id,customer,product,category,quantity,unitPrice,status,timestamp"];

for (let i = 0; i < 50000; i++) {
    const cat = categories[i % 5];
    const stat = statuses[i % 5];
    const qty = (i % 10) + 1;
    const price = ((i % 200) + 5) * 1.5;
    rows.push(`${i},Customer_${i % 1000},Product_${i % 500},${cat},${qty},${price},${stat},2026-08-21T12:00:00Z`);
}

const csvText = rows.join("\n");

// 2. Real-world Data Ingestion, Filtering, Aggregation and Serialization Pipeline
const parsed = parseCSV(csvText);

const completedSales: any[] = [];
const categoryRevenue = new Map<string, number>();
let totalRevenue = 0.0;

for (let i = 0; i < parsed.length; i++) {
    const row = parsed[i];
    if (row.status === "COMPLETED") {
        const lineTotal = Number(row.quantity) * Number(row.unitPrice);
        totalRevenue += lineTotal;

        const catName = String(row.category);
        const currentRev = categoryRevenue.get(catName) || 0;
        categoryRevenue.set(catName, currentRev + lineTotal);

        completedSales.push(row);
    }
}

const serializedExport = stringifyCSV(completedSales);

console.log(`CSV Pipeline: Completed Sales = ${completedSales.length}, Total Revenue = $${totalRevenue}, Export bytes = ${serializedExport.length}`);
