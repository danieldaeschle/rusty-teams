(function(){
  var d=document,root=d.documentElement;
  function $(s,c){return (c||d).querySelector(s)}
  function $$(s,c){return Array.prototype.slice.call((c||d).querySelectorAll(s))}
  var reduce=window.matchMedia&&matchMedia("(prefers-reduced-motion:reduce)").matches;

  // theme toggle
  var tb=$("#theme");
  if(tb)tb.addEventListener("click",function(){
    var dark=root.dataset.theme?root.dataset.theme==="dark":matchMedia("(prefers-color-scheme:dark)").matches;
    var next=dark?"light":"dark";
    if(!reduce){root.classList.add("theme-anim");setTimeout(function(){root.classList.remove("theme-anim")},450)}
    root.dataset.theme=next;
    try{localStorage.setItem("theme",next)}catch(e){}
  });

  // mobile menu
  var bg=$("#burger"),menu=$("#menu");
  if(bg)bg.addEventListener("click",function(){
    var o=menu.classList.toggle("open");bg.setAttribute("aria-expanded",o)});
  $$("#menu a").forEach(function(a){a.addEventListener("click",function(){menu.classList.remove("open");bg.setAttribute("aria-expanded","false")})});

  // reveals + rule draw
  function onceIn(els,cb){
    if(!("IntersectionObserver" in window)){els.forEach(cb);return}
    var io=new IntersectionObserver(function(es){es.forEach(function(e){
      if(e.isIntersecting){cb(e.target);io.unobserve(e.target)}})},{rootMargin:"0px 0px -12% 0px"});
    els.forEach(function(e){io.observe(e)});
  }
  onceIn($$("[data-reveal]"),function(el){
    var sib=el.parentNode.children;
    var i=Array.prototype.indexOf.call(sib,el);
    el.style.transitionDelay=(i%4)*0.08+"s";
    el.classList.add("in");
    setTimeout(function(){el.style.transitionDelay=""},1300);
  });
  onceIn($$(".section-rule"),function(el){el.classList.add("in")});

  // OS detection
  var ua=(navigator.userAgent||"")+" "+(navigator.platform||"");
  var os=/Android|iPhone|iPad/.test(ua)?null:/Win/i.test(ua)?"windows":/Mac/i.test(ua)?"mac":/Linux|X11/i.test(ua)?"linux":null;
  var names={windows:"Windows",linux:"Linux",mac:"macOS"};
  if(os){
    var card=$('[data-os="'+os+'"]');
    if(card){
      card.classList.add("yours");
      var pl=$(".plabel",card);pl.textContent="Your system";pl.classList.add("yl");
      var b=$("[data-primary]",card);b.classList.remove("btn-out");b.classList.add("btn-solid");
    }
    var h=$("#os-hint");if(h)h.textContent="Detected "+names[os]+" · also Windows, Linux, macOS";
    var nt=$("#os-note");if(nt)nt.textContent="Your system: "+names[os];
  }

  // copy
  function flash(el,label){var o=el.textContent;el.textContent=label;setTimeout(function(){el.textContent=o},1400)}
  var cc=$("#copy-cmd");
  if(cc)cc.addEventListener("click",function(){
    if(navigator.clipboard)navigator.clipboard.writeText($("#cmd").textContent).then(function(){flash($("span",cc),"Copied")},function(){});
  });
  var cl=$("#copy-link");
  if(cl)cl.addEventListener("click",function(){
    if(navigator.clipboard)navigator.clipboard.writeText(location.href.split("#")[0]).then(function(){flash(cl.lastChild,"Copied")},function(){});
  });

  // ruler + breadcrumb
  var ruler=$("#ruler");
  if(!ruler||getComputedStyle(ruler).display==="none")return;
  var band=$("#band"),needle=$("#needle"),pct=$("#pct");
  for(var i=0;i<=40;i++){
    var m=d.createElement("span");m.className="mk"+(i%4===0?" maj":"");m.style.top=(i/40*100)+"%";
    if(i%4===0){var t=d.createElement("i");t.textContent=i*2.5;m.appendChild(t)}
    ruler.insertBefore(m,band);
  }
  var secs=$$("section[data-name]"),cur=$("#crumb-cur"),prev=$("#crumb-prev");
  function update(){
    var H=root.scrollHeight,vh=innerHeight,max=Math.max(1,H-vh),p=Math.min(1,Math.max(0,scrollY/max));
    needle.style.top=(p*100)+"%";
    band.style.top=(scrollY/H*100)+"%";band.style.height=(vh/H*100)+"%";
    var v=Math.round(p*100);pct.textContent=("00"+v).slice(-3)+" %";
    ruler.setAttribute("aria-valuenow",v);
    var idx=0;secs.forEach(function(s,k){if(s.getBoundingClientRect().top<vh*0.4)idx=k});
    if(cur.textContent!==secs[idx].dataset.name){cur.textContent=secs[idx].dataset.name;prev.textContent=idx?secs[idx-1].dataset.name:""}
  }
  addEventListener("scroll",update,{passive:true});addEventListener("resize",update);update();
  ruler.addEventListener("click",function(e){
    scrollTo({top:e.clientY/innerHeight*(root.scrollHeight-innerHeight),behavior:reduce?"auto":"smooth"});
  });
})();
