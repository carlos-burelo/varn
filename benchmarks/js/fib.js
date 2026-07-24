function fib(n){ if(n<=1) return n; return fib(n-1)+fib(n-2); }
function compute(){ return fib(35); }
for(let w=0;w<3;w++) compute();
let best=Infinity, chk;
for(let r=0;r<10;r++){ const t=performance.now(); chk=compute(); const ms=performance.now()-t; if(ms<best)best=ms; }
console.error(chk); console.log(best.toFixed(3));
