// Actual production Vue/IPC, synthetic account and libmpv/null output.
(async () => {
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
  const one = selector => document.querySelector(selector);
  const all = selector => [...document.querySelectorAll(selector)];
  const assert = (ok, message) => { if (!ok) throw Error(message); };
  const wait = async (test, label) => { for (let i=0;i<180;i++) { if(test()) return; await sleep(100); } throw Error(label); };
  const emit = (stage, values={}) => window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {event:'ux-proof',payload:{stage,...values}});
  const click = node => { assert(node&&!node.disabled, `unavailable ${node?.outerHTML}`); node.focus({preventScroll:true}); node.click(); };
  const button = (label, root=document) => [...root.querySelectorAll('button')].find(node=>node.getAttribute('aria-label')===label||node.textContent.trim()===label);
  const key = (node,value) => node.dispatchEvent(new KeyboardEvent('keydown',{key:value,bubbles:true,cancelable:true}));
  const rect = node => {const r=node.getBoundingClientRect();return {x:r.x,y:r.y,right:r.right,bottom:r.bottom,width:r.width,height:r.height};};
  const inside = (a,b) => a.x>=b.x-1&&a.y>=b.y-1&&a.right<=b.right+1&&a.bottom<=b.bottom+1;
  const hit = node => {const r=rect(node),at=document.elementFromPoint(r.x+r.width/2,r.y+r.height/2);return node===at||node.contains(at);};
  let stageCount=0;
  const checks=[];
  async function theme(value) {
    click(button('外观主题'));await wait(()=>one('.action-menu'),'appearance');
    click(button(value==='dark'?'深色':'浅色'));await wait(()=>document.documentElement.dataset.theme===value&&!one('.action-menu'),'appearance applied');
  }
  async function openId(id, waitForPage=true) {
    click(button('我的歌单'));await wait(()=>button('打开歌单链接'),'library');
    click(button('打开歌单链接'));await wait(()=>one('[aria-label="歌单分享链接"]'),'link dialog');
    const input=one('[aria-label="歌单分享链接"]');input.value=String(id);input.dispatchEvent(new Event('input',{bubbles:true}));
    click(one('.playlist-dialog button.primary'));
    await wait(()=>!one('.playlist-dialog'),'link dialog closed');
    if(waitForPage) await wait(()=>one('.playlist-hero h1')?.textContent.trim()===(id===1?'我喜欢的音乐':id===2?'午后散步 · 一份用来检查较长名称与编辑入口的歌单':`合成歌单 ${id}`),'playlist loaded');
  }
  async function capture(label, sourceLayout=false) {
    await sleep(350);
    const hero=one('.playlist-hero'),summary=one('.playlist-summary'),actions=one('.playlist-actions');
    const overlay=one('.playlist-description-overlay'),modal=one('dialog[open]'),menu=one('.action-menu');
    const box={x:0,y:0,right:innerWidth,bottom:innerHeight};
    const sourceChecks=!sourceLayout?{}:{
      actionsInInformation:!!summary&&!!actions&&inside(rect(actions),rect(summary)),
      descriptionBeforeCredits:!!one('.playlist-description-preview')&&rect(one('.playlist-description-preview')).bottom<=rect(one('.playlist-credits')).y,
      uniformActions:!!actions&&all('.playlist-actions > button,.playlist-actions .menu-trigger').every(n=>rect(n).height===(innerHeight<=650?36:40)&&getComputedStyle(n).borderRadius==='8px'),
      noDuplicateActions:!one('.track-actions'),
    };
    const values={label,width:innerWidth,height:innerHeight,theme:document.documentElement.dataset.theme,
      hero:hero&&rect(hero),summary:summary&&rect(summary),actions:actions&&rect(actions),renderedTracks:all('.track-row').length,
      overlay:overlay&&{...rect(overlay),scrollTop:one('.playlist-description-scroll').scrollTop,contentHeight:one('.playlist-description-scroll').scrollHeight},
      checks:{noOverflow:document.documentElement.scrollWidth<=innerWidth,
        rootAnchored:rect(document.body).x===0&&rect(document.body).y===0,
        playerSeparated:!hero||rect(hero).bottom<=rect(one('.playerbar')).y,
        heroContained:!hero||inside(rect(hero),box),
        controlsHit:!!(modal||overlay||menu)||all('.playlist-actions button:not(:disabled),.playlist-edit:not(:disabled)').every(hit),
        overlayContained:!overlay||inside(rect(overlay),rect(summary)),
        descriptionWrap:!overlay||one('.playlist-description-scroll').scrollWidth<=one('.playlist-description-scroll').clientWidth+1,
        boundedRows:all('.track-row').length<40,
        ...sourceChecks}};
    checks.push({label,...values.checks});
    await emit(`ux_${String(++stageCount).padStart(2,'0')}_${label}`,values);await sleep(800);
  }
  try {
    await wait(()=>all('.track-row').length,'daily');
    await wait(()=>button('列表循环')&&!button('列表循环').disabled,'native player ready');
    await openId(2);
    for(const [width,height,value] of [[1120,760,'light'],[760,540,'light'],[760,540,'dark'],[1120,760,'dark']]) {
      await emit('resize',{width,height});await wait(()=>innerWidth===width&&innerHeight===height,'resize');await theme(value);
      const suffix=`${width}_${value}`;
      await capture(`owned_${suffix}`,true);
      if(window.__playlistLayoutBaseline) continue;
      click(button('展开歌单简介'));await wait(()=>one('.playlist-description-overlay'),'description');
      assert(document.activeElement===button('收起歌单简介'),'description focus');
      await capture(`description_${suffix}`);
      const scroll=one('.playlist-description-scroll');scroll.focus();scroll.scrollTop=scroll.scrollHeight;await capture(`description_end_${suffix}`);
      assert(scroll.scrollTop>0&&scroll.textContent.includes('简介结束标记'),'full description must remain readable');
      key(scroll,'Escape');await wait(()=>!one('.playlist-description-overlay'),'description escape');
      assert(document.activeElement===button('展开歌单简介'),'description return focus');
      click(button('更多歌单操作'));await wait(()=>one('.action-menu'),'header menu');await capture(`menu_${suffix}`);
      click(button('编辑歌单',one('.action-menu')));await wait(()=>one('[aria-label="歌单名称"]')?.value,'editor');await capture(`editor_${suffix}`);
      click(button('关闭歌单操作'));await wait(()=>!one('.playlist-dialog'),'editor closed');await sleep(200);
      assert(document.activeElement===button('更多歌单操作'),'menu editor return focus');
      click(button('展开歌单简介'));await wait(()=>one('.playlist-description-overlay'),'description reopen');
      one('.searchbox input').focus();await wait(()=>!one('.playlist-description-overlay'),'description closes on focus outside');
    }
    if(!window.__playlistLayoutBaseline) {
      for(const [width,height,value] of [[1120,760,'light'],[760,540,'dark']]) {
        await emit('resize',{width,height});await wait(()=>innerWidth===width&&innerHeight===height,'state resize');await theme(value);
        for(const [id,label] of [[3,'short'],[4,'empty'],[1,'favorite'],[20,'subscribed'],[25,'unlisted']]) {
          await openId(id);await capture(`${label}_${width}_${value}`);
          if(id===3) assert(!button('展开歌单简介'),'short description needs no expander');
          if(id===4) assert(!one('.playlist-description')&&button('播放全部').disabled&&button('下载当前歌曲列表').disabled,'empty playlist actions');
          if(id===1) assert(!one('.playlist-edit')&&!button('更多歌单操作'),'favorite has no editor');
          if(id===20||id===25) {
            assert(!one('.playlist-edit'),'cannot edit another creator');
            assert(button(id===20?'取消收藏歌单':'收藏歌单')?.getAttribute('aria-pressed')===(id===20?'true':'false'),'subscription display');
          }
          if(id===25) {
            assert(!button('更多歌单操作'),'unlisted playlist must not expose inapplicable reorder actions');
          }
          if(id===20) {
            click(button('更多歌单操作'));await wait(()=>one('.action-menu'),'disabled menu');
            assert(all('.action-menu button').every(n=>n.disabled),'single collected list cannot be reordered');
            assert(document.activeElement===one('.action-menu'),'all-disabled menu must keep keyboard focus');
            const focusStyle=getComputedStyle(one('.action-menu'));
            assert(focusStyle.outlineStyle==='solid'&&focusStyle.outlineWidth==='2px'&&focusStyle.outlineOffset==='-2px','disabled menu must use an explicit inset focus ring');
            await capture(`disabled_menu_${width}_${value}`);
            key(document.activeElement,'Escape');await wait(()=>!one('.action-menu'),'disabled menu escape');
            assert(document.activeElement===button('更多歌单操作'),'disabled menu return focus');
          }
        }
      }
      await openId(26,false);await wait(()=>one('.empty-state .spinner'),'loading state');await capture('loading');
      await emit('playlist-fixture-release');
      await wait(()=>one('.error-state'),'read failure');
      assert(!one('.page-heading').textContent.includes('正在'),'failed page must not keep announcing loading');
      await capture('read_failed');
      click(button('重试'));await wait(()=>one('.playlist-hero h1')?.textContent==='合成歌单 26','retry recovered');await capture('read_recovered');
      click(button('歌单评论'));await wait(()=>one('.comments-page .error-state'),'comment error presentation');await capture('comments_failure');
      click(button('返回上一页'));await wait(()=>one('.playlist-hero h1')?.textContent==='合成歌单 26','comments return');
      await openId(5);assert(one('.track-play').disabled,'unavailable first track fixture');
      click(button('播放全部'));await wait(()=>one('.playerbar').dataset.state==='playing'&&Number(one('.playerbar').dataset.position)>.15,'header starts real synthetic audio');
      click(button('暂停'));await wait(()=>one('.playerbar').dataset.state==='paused','native paused');
      assert(one('.now-copy')?.textContent.includes('合成歌曲 2'),'play all must start at first available track');
      await capture('unavailable_first_track');
      await openId(2);click(button('展开歌单简介'));await wait(()=>one('.playlist-description-scroll'),'keyboard reader');
      const reader=one('.playlist-description-scroll');reader.focus();
      for(const shiftKey of [false,true]) {
        const event=new KeyboardEvent('keydown',{key:' ',code:'Space',shiftKey,bubbles:true,cancelable:true});
        reader.dispatchEvent(event);await sleep(250);
        assert(!event.defaultPrevented&&one('.playerbar').dataset.state==='paused','reader Space must not toggle audio or suppress native scrolling');
      }
      await capture('description_keyboard');
      key(reader,'Escape');await wait(()=>!one('.playlist-description-overlay'),'keyboard reader close');
      assert(document.activeElement===button('展开歌单简介'),'reader keyboard return focus');
    }
    const passed=checks.every(row=>Object.entries(row).every(([key,value])=>key==='label'||value===true));
    if(!window.__playlistLayoutBaseline) assert(passed,JSON.stringify(checks));
    await emit('complete',{ok:true,stageCount,checks,layoutPassed:passed,realMusicRequests:0,accountWrites:0});
  } catch(error) {await emit('failed',{message:String(error),stack:error?.stack,stageCount});}
})();
