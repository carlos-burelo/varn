const { performance } = require('perf_hooks');
const report = (n, c, ms) => console.log(`${n} | checksum=${c} | ms=${ms}`);
const concatAppend = () => { let s = ''; for (let i = 0; i < 20000; i++) s += 'abc'; return s.length; };
const concatParts = () => { let t = 0; for (let i = 0; i < 200000; i++) { const p = 'a' + i + 'b'; t += p.length; } return t; };
const join = () => { const p = []; for (let i = 0; i < 100000; i++) p.push('item' + i); return p.join(',').length; };
const search = () => { const h = 'abcdefghij'.repeat(1000) + 'needle'; let t = 0; for (let i = 0; i < 2000; i++) { t += h.indexOf('needle') ?? -1; if (h.includes('efgh')) t++; } return t; };
const split = () => { const p = []; for (let i = 0; i < 10000; i++) p.push(String(i)); const csv = p.join(','); let t = 0; for (let r = 0; r < 100; r++) t += csv.split(',').length; return t; };
const slice = () => { const b = 'abcdefghijklmnopqrstuvwxyz'.repeat(400); let t = 0; for (let i = 0; i < 200000; i++) { const s = i % 10000; t += b.substring(s, s + 10).length; } return t; };
const kase = () => { const b = 'AbCdEfGhIj'.repeat(1000); let t = 0; for (let i = 0; i < 500; i++) { t += b.toUpperCase().length; t += b.toLowerCase().length; } return t; };
const replaceAll = () => { const b = 'the quick brown fox jumps over the lazy dog '.repeat(200); let t = 0; for (let i = 0; i < 200; i++) t += b.replaceAll('o', '00').length; return t; };
const prefix = () => { const it = []; for (let i = 0; i < 100000; i++) it.push('item' + i + 'x'); let c = 0; for (let j = 0; j < 100000; j++) { if (it[j].startsWith('item1')) c++; if (it[j].endsWith('9x')) c++; } return c; };
const charCode = () => { const b = 'abcdefghijklmnopqrstuvwxyz0123456789'.repeat(2000); let t = 0; for (let r = 0; r < 20; r++) for (let i = 0; i < b.length; i++) t += b.charCodeAt(i); return t; };
const equality = () => { const it = []; for (let i = 0; i < 1000; i++) it.push('key' + (i % 100)); let c = 0; for (let r = 0; r < 200; r++) for (let j = 0; j < 1000; j++) if (it[j] === 'key50') c++; return c; };
const intToStr = () => { let t = 0; for (let i = 0; i < 200000; i++) t += String(i).length; return t; };
for (const [n, f] of [['concat_append', concatAppend], ['concat_parts', concatParts], ['join', join], ['search', search], ['split', split], ['slice', slice], ['case', kase], ['replace_all', replaceAll], ['prefix_suffix', prefix], ['char_code', charCode], ['equality', equality], ['int_to_str', intToStr]]) {
    const t0 = performance.now();
    report(n, f(), performance.now() - t0);
}
