function benchMath(it){ let r=1.0,i=0; while(i<it){ r=Math.abs(r-i); r=Math.sqrt(r+1.0); r=Math.floor(r*10.0)/10.0; i=i+1; } return r; }
function compute(){ return benchMath(500000); }
for(let w=0;w<3;w++) compute();
let best=Infinity, chk;
for(let r=0;r<10;r++){ const t=performance.now(); chk=compute(); const ms=performance.now()-t; if(ms<best)best=ms; }
console.error(chk); console.log(best.toFixed(3));
