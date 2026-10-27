(async()=>{
 const sleep=ms=>new Promise(r=>setTimeout(r,ms));
 const emit=(stage,data={})=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'shell-proof',payload:{stage,...data}});
 const all=s=>[...document.querySelectorAll(s)];
 const wait=async(fn)=>{for(let i=0;i<200;i++){if(fn())return;await sleep(100);}throw Error('shell_timeout');};
 function bounds(n){const r=n.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};}
 function facts(){return{width:innerWidth,height:innerHeight,overflow:document.documentElement.scrollWidth>innerWidth,
  navigation:all('.sidebar .nav-item').map(n=>{const s=getComputedStyle(n);const text=[...n.childNodes].filter(c=>c.nodeType===Node.TEXT_NODE).map(c=>{const r=document.createRange();r.selectNodeContents(c);return{text:c.textContent.trim(),rects:[...r.getClientRects()].map(b=>({x:b.x,y:b.y,width:b.width,height:b.height}))};});return{text:n.textContent.trim(),box:bounds(n),color:s.color,opacity:s.opacity,visibility:s.visibility,display:s.display,font:s.font,fontSize:s.fontSize,transform:s.transform,clipPath:s.clipPath,contentVisibility:s.contentVisibility,textBoxes:text,icon:n.querySelector('svg')?bounds(n.querySelector('svg')):null};})};}
 async function capture(stage){await sleep(800);await emit(stage,facts());await sleep(1200);}
 try{
  await wait(()=>document.querySelector('.welcome'));await capture('shell_initial');
  all('.nav-item').find(n=>n.textContent.trim()==='本地音乐').click();await wait(()=>document.querySelector('.local-page'));await capture('shell_local');
  await emit('resize',{width:1120,height:760});await wait(()=>innerWidth===1120);await capture('shell_wide');
  await emit('resize',{width:760,height:540});await wait(()=>innerWidth===760);await capture('shell_narrow');
  document.querySelector('.sidebar').style.transform='translateZ(0)';await capture('shell_repaint');
  await emit('complete',{ok:true,networkRequests:0});
 }catch(e){await emit('failed',{ok:false,error:e instanceof Error?e.message:'shell_error'});}
})();
