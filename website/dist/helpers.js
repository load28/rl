(function(){
var H=window.__h={};

H.e=function(t){return document.createElement(t)};
H.t=function(s){return document.createTextNode(s)};
H.f=function(){return document.createDocumentFragment()};

H.ins=function(t,n,a){t.insertBefore(n,a||null)};
H.det=function(n){if(n.parentNode)n.parentNode.removeChild(n)};
H.app=function(t,n){t.appendChild(n)};

H.tx=function(n,s){n.textContent=s};
H.esc=function(s){var d=H.e('div');d.textContent=s;return d.innerHTML};
H.safeUrl=function(u){return/^(https?:\/\/|\/|#|mailto:)/.test(u)?u:'about:blank'};

H.at=function(n,k,v){if(v==null)n.removeAttribute(k);else n.setAttribute(k,v)};

var dg={},DE=['click','dblclick','mousedown','mouseup','input','change',
  'keydown','keyup','focusin','focusout','submit','pointerdown','pointerup',
  'touchstart','touchend','contextmenu'];
H.delegate=function(t){if(dg[t])return;dg[t]=1;
  document.addEventListener(t,function(ev){var n=ev.target;
    while(n){var h=n.__ev&&n.__ev[t];
      if(h){h.call(n,ev);if(ev.cancelBubble)break}n=n.parentNode}})};
H.on=function(n,t,fn){
  if(DE.indexOf(t)>=0){H.delegate(t);if(!n.__ev)n.__ev={};n.__ev[t]=fn;
    return function(){delete n.__ev[t]}}
  n.addEventListener(t,fn);return function(){n.removeEventListener(t,fn)}};

H.signal=function(v){var s=[];return{
  get:function(){return v},
  set:function(n){if(n===v)return;v=n;for(var i=0;i<s.length;i++)s[i](v)},
  sub:function(fn){s.push(fn);return function(){s=s.filter(function(f){return f!==fn})}}}};
H.derived=function(deps,fn){var sig=H.signal(fn());
  var up=function(){sig.set(fn())};
  var us=deps.map(function(d){return d.sub(up)});
  sig.unsub=function(){us.forEach(function(u){u()})};return sig};

H.dirty=function(m,b){return(m&b)!==0};
H.mark=function(m,b){return m|b};

H.component=function(factory){return function(ctx){var f=factory(ctx);
  return{c:f.c,m:f.m,p:f.p||function(){},d:f.d}}};

H.disposer=function(){var f=[];return{
  add:function(fn){f.push(fn)},
  flush:function(){for(var i=0;i<f.length;i++)f[i]();f=[]}}};

H.fetcher=function(){var c=new AbortController();return{
  signal:c.signal,abort:function(){c.abort()},
  fetch:function(u,o){return fetch(u,Object.assign({},o,{signal:c.signal}))}}};

H.scope=function(el,id){el.classList.add('s-'+id)};

var pend=false,q=[];
H.tick=function(fn){q.push(fn);if(!pend){pend=true;
  queueMicrotask(function(){pend=false;var a=q;q=[];
    for(var i=0;i<a.length;i++)a[i]()})}};

H.transition=function(node,cfg){
  cfg=cfg||{};var dur=cfg.duration||300;
  var ease=cfg.easing||function(t){return t};
  var raf=null;
  function run(fn,from,to){
    return new Promise(function(resolve){
      var start=null;if(raf)cancelAnimationFrame(raf);
      function step(ts){
        if(!start)start=ts;var p=Math.min((ts-start)/dur,1);
        var t=from<to?ease(p):ease(1-p);
        fn(t,node);
        if(p<1){raf=requestAnimationFrame(step)}
        else{raf=null;resolve()}
      }
      raf=requestAnimationFrame(step)
    })
  }
  return{
    intro:function(){return run(cfg.intro||function(){},0,1)},
    outro:function(){return run(cfg.outro||function(){},1,0)},
    destroy:function(){if(raf)cancelAnimationFrame(raf)}
  }
};

H.boundary=function(container,tryFn,fallbackFn){
  var cleanup=null;
  try{
    var result=tryFn();
    if(result&&typeof result.then==='function'){
      result.catch(function(err){
        container.innerHTML='';
        cleanup=fallbackFn(err,container);
      })
    }
  }catch(err){
    container.innerHTML='';
    cleanup=fallbackFn(err,container);
  }
  return function(){if(cleanup&&typeof cleanup==='function')cleanup()}
};

H.inview=function(node,opts){
  opts=opts||{};
  var visible=H.signal(false);var ratio=H.signal(0);
  var obs=new IntersectionObserver(function(entries){
    var e=entries[0];visible.set(e.isIntersecting);ratio.set(e.intersectionRatio)
  },{threshold:opts.threshold||0,rootMargin:opts.rootMargin||'0px'});
  obs.observe(node);
  return{visible:visible,ratio:ratio,destroy:function(){obs.disconnect()}}
};

H.spring=function(val,cfg){
  cfg=cfg||{};var st=cfg.stiffness||0.15,dm=cfg.damping||0.8,pr=cfg.precision||0.01;
  var cur=val,tgt=val,vel=0,subs=[],raf=null;
  function notify(){for(var i=0;i<subs.length;i++)subs[i](cur)}
  function tick(){
    var f=st*(tgt-cur);vel=(vel+f)*dm;cur+=vel;
    if(Math.abs(tgt-cur)<pr&&Math.abs(vel)<pr){cur=tgt;vel=0;raf=null;notify();return}
    notify();raf=requestAnimationFrame(tick)
  }
  return{
    get:function(){return cur},
    set:function(v){tgt=v;if(!raf)raf=requestAnimationFrame(tick)},
    sub:function(fn){subs.push(fn);return function(){subs=subs.filter(function(f){return f!==fn})}},
    destroy:function(){if(raf)cancelAnimationFrame(raf);subs=[]}
  }
};

H.tweened=function(val,cfg){
  cfg=cfg||{};var dur=cfg.duration||400;
  var ease=cfg.easing||function(t){return t};
  var cur=val,from=val,tgt=val,subs=[],raf=null,st=null;
  function notify(){for(var i=0;i<subs.length;i++)subs[i](cur)}
  function tick(ts){
    if(!st)st=ts;var t=Math.min((ts-st)/dur,1);
    cur=from+(tgt-from)*ease(t);notify();
    if(t<1){raf=requestAnimationFrame(tick)}else{raf=null}
  }
  return{
    get:function(){return cur},
    set:function(v){from=cur;tgt=v;st=null;if(!raf)raf=requestAnimationFrame(tick)},
    sub:function(fn){subs.push(fn);return function(){subs=subs.filter(function(f){return f!==fn})}},
    destroy:function(){if(raf)cancelAnimationFrame(raf);subs=[]}
  }
};
})();
