class User{ constructor(id,name,age,isActive,score){ this.id=id;this.name=name;this.age=age;this.isActive=isActive;this.score=score; } }
class UserDTO{ constructor(id,displayName,score){ this.id=id;this.displayName=displayName;this.score=score; } }
function compute(){
  const count=100000; const users=[];
  for(let i=0;i<count;i++) users.push(new User(i,"User_"+i,(i%60)+10,(i%2)===0,(i%100)*1.5));
  const aa=[]; let ts=0.0;
  for(let i=0;i<count;i++){ const u=users[i]; if(u.isActive&&u.age>=18){ aa.push(new UserDTO(u.id,u.name,u.score)); ts=ts+u.score; } }
  return aa.length+ts;
}
for(let w=0;w<3;w++) compute();
let best=Infinity, chk;
for(let r=0;r<10;r++){ const t=performance.now(); chk=compute(); const ms=performance.now()-t; if(ms<best)best=ms; }
console.error(chk); console.log(best.toFixed(3));
