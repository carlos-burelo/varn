// String operations micro-benchmark. Paired with bench_str_ops.vn.
// Each section prints: name, checksum, time (ms). Checksums must match the Varn side.

function report(name: string, checksum: number, ms: number) {
    console.log(name + " | checksum=" + checksum.toString() + " | ms=" + ms.toString());
}

// 1. Append concat: s = s + piece, N times
function benchConcatAppend(): number {
    let s = "";
    let i = 0;
    while (i < 20000) {
        s = s + "abc";
        i = i + 1;
    }
    return s.length;
}

// 2. Small concat parts: allocation-heavy short-lived strings
function benchConcatParts(): number {
    let total = 0;
    let i = 0;
    while (i < 200000) {
        const piece = "a" + i.toString() + "b";
        total = total + piece.length;
        i = i + 1;
    }
    return total;
}

// 3. Array build + join
function benchJoin(): number {
    const parts: string[] = [];
    let i = 0;
    while (i < 100000) {
        parts.push("item" + i.toString());
        i = i + 1;
    }
    const joined = parts.join(",");
    return joined.length;
}

// 4. indexOf + includes over a large haystack
function benchSearch(): number {
    const haystack = "abcdefghij".repeat(1000) + "needle";
    let total = 0;
    let i = 0;
    while (i < 2000) {
        total = total + haystack.indexOf("needle");
        if (haystack.includes("efgh")) {
            total = total + 1;
        }
        i = i + 1;
    }
    return total;
}

// 5. split on a big CSV line
function benchSplit(): number {
    const parts: string[] = [];
    let i = 0;
    while (i < 10000) {
        parts.push(i.toString());
        i = i + 1;
    }
    const csv = parts.join(",");
    let total = 0;
    let r = 0;
    while (r < 100) {
        const fields = csv.split(",");
        total = total + fields.length;
        r = r + 1;
    }
    return total;
}

// 6. substring/slice windows over a large string
function benchSlice(): number {
    const base = "abcdefghijklmnopqrstuvwxyz".repeat(400);
    let total = 0;
    let i = 0;
    while (i < 200000) {
        const start = i % 10000;
        const sub = base.substring(start, start + 10);
        total = total + sub.length;
        i = i + 1;
    }
    return total;
}

// 7. toUpperCase / toLowerCase on a 10k-char string
function benchCase(): number {
    const base = "AbCdEfGhIj".repeat(1000);
    let total = 0;
    let i = 0;
    while (i < 500) {
        total = total + base.toUpperCase().length;
        total = total + base.toLowerCase().length;
        i = i + 1;
    }
    return total;
}

// 8. replaceAll with growth
function benchReplaceAll(): number {
    const base = "the quick brown fox jumps over the lazy dog ".repeat(200);
    let total = 0;
    let i = 0;
    while (i < 200) {
        total = total + base.replaceAll("o", "00").length;
        i = i + 1;
    }
    return total;
}

// 9. startsWith / endsWith over many strings
function benchPrefixSuffix(): number {
    const items: string[] = [];
    let i = 0;
    while (i < 100000) {
        items.push("item" + i.toString() + "x");
        i = i + 1;
    }
    let count = 0;
    let j = 0;
    while (j < 100000) {
        if (items[j].startsWith("item1")) {
            count = count + 1;
        }
        if (items[j].endsWith("9x")) {
            count = count + 1;
        }
        j = j + 1;
    }
    return count;
}

// 10. charCodeAt scan
function benchCharCode(): number {
    const base = "abcdefghijklmnopqrstuvwxyz0123456789".repeat(2000);
    let total = 0;
    let r = 0;
    while (r < 20) {
        let i = 0;
        const len = base.length;
        while (i < len) {
            total = total + base.charCodeAt(i);
            i = i + 1;
        }
        r = r + 1;
    }
    return total;
}

// 11. string equality comparisons
function benchEquality(): number {
    const items: string[] = [];
    let i = 0;
    while (i < 1000) {
        items.push("key" + (i % 100).toString());
        i = i + 1;
    }
    let count = 0;
    let r = 0;
    while (r < 200) {
        let j = 0;
        while (j < 1000) {
            if (items[j] === "key50") {
                count = count + 1;
            }
            j = j + 1;
        }
        r = r + 1;
    }
    return count;
}

// 12. int -> str conversion
function benchIntToStr(): number {
    let total = 0;
    let i = 0;
    while (i < 200000) {
        total = total + i.toString().length;
        i = i + 1;
    }
    return total;
}

function runSection(name: string, f: () => number) {
    const start = performance.now();
    const checksum = f();
    const elapsed = performance.now() - start;
    report(name, checksum, elapsed);
}

runSection("concat_append", benchConcatAppend);
runSection("concat_parts", benchConcatParts);
runSection("join", benchJoin);
runSection("search", benchSearch);
runSection("split", benchSplit);
runSection("slice", benchSlice);
runSection("case", benchCase);
runSection("replace_all", benchReplaceAll);
runSection("prefix_suffix", benchPrefixSuffix);
runSection("char_code", benchCharCode);
runSection("equality", benchEquality);
runSection("int_to_str", benchIntToStr);
