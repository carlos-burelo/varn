function compute(){
  const n=150,size=n*n; const a=[],b=[],c=[];
  for(let i=0;i<size;i++){ a.push((i%100)+1); b.push(((i*3)%100)+1); c.push(0); }
  for(let row=0;row<n;row++) for(let col=0;col<n;col++){ let s=0; for(let k=0;k<n;k++) s=s+a[row*n+k]*b[k*n+col]; c[row*n+col]=s; }
  return c[0]+c[size-1];
}
for(let w=0;w<3;w++) compute();
let best=Infinity, chk;
for(let r=0;r<10;r++){ const t=performance.now(); chk=compute(); const ms=performance.now()-t; if(ms<best)best=ms; }
console.error(chk); console.log(best.toFixed(3));
