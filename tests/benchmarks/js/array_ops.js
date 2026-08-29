function compute(){
  const arr=[]; const size=500000; let i=0;
  while(i<size){ arr.push(i); i=i+1; }
  let sum=0,j=0; while(j<size){ sum=sum+arr[j]; j=j+1; }
  let k=0; while(k<size){ arr[k]=k*2; k=k+1; }
  return sum;
}
for(let w=0;w<3;w++) compute();
let best=Infinity, chk;
for(let r=0;r<10;r++){ const t=performance.now(); chk=compute(); const ms=performance.now()-t; if(ms<best)best=ms; }
console.error(chk); console.log(best.toFixed(3));
