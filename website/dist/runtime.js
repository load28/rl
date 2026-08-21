(function(){
  var a={},b={},c={};
  window.__a=a;
  window.__b=b;
  window.__r=function(d,e){a[d]=e};
  window.__i=function(d){
    var vendorMap=window.__vendorMap||{};
    var origId=d;
    if(vendorMap[d])d=vendorMap[d];
    if(b[d]){
      var exp=b[d].exports;
      if(origId!==d&&exp[origId])return exp[origId];
      return exp
    }
    if(!a[d]){
      if(c[d])return c[d];
      c[d]=fetch("chunks/"+d).then(function(r){return r.text()})
        .then(function(t){new Function(t)();delete c[d];return window.__i(origId)});
      return c[d]
    }
    var m=b[d]={exports:{}};
    a[d](m.exports,window.__i,m);
    var exp=m.exports;
    if(origId!==d&&exp[origId])return exp[origId];
    return exp
  };
})();
