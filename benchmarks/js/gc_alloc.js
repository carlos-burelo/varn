class GcVtA{ constructor(x){ this.x=x; } }
class GcVtB{ constructor(y){ this.y=y; } }
function compute(){
  const junk=[]; for(let i=0;i<400000;i++) junk.push("gc_"+i);
  let aa=0,bb=0; for(let i=0;i<100000;i++){ const a=new GcVtA(i); aa+=a.x; const b=new GcVtB(i); bb+=b.y; }
  return aa+bb+junk.length;
}
for(let w=0;w<3;w++) compute();
let best=Infinity, chk;
for(let r=0;r<10;r++){ const t=performance.now(); chk=compute(); const ms=performance.now()-t; if(ms<best)best=ms; }
console.error(chk); console.log(best.toFixed(3));
