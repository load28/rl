(function() {
  var ws = new WebSocket('ws://' + location.host);
  var disposeCallbacks = {};
  ws.onmessage = function(ev) {
    var msg = JSON.parse(ev.data);
    if (msg.type === 'full-reload') location.reload();
    if (msg.type !== 'update') return;
    msg.updates.forEach(function(update) {
      if (update.type !== 'js-update') return;
      var chunkId = update.path;
      if (disposeCallbacks[chunkId]) disposeCallbacks[chunkId]();
      if (window.__b) delete window.__b[chunkId];
      if (window.__a) delete window.__a[chunkId];
      var script = document.createElement('script');
      script.src = 'chunks/' + chunkId + '?t=' + update.timestamp;
      script.onload = function() {
        document.head.removeChild(script);
        var mod = window.__i(chunkId);
        if (mod && mod.__cleanup) disposeCallbacks[chunkId] = mod.__cleanup;
        if (mod && mod.mount && window.__hmr_remount) window.__hmr_remount(chunkId, mod);
      };
      document.head.appendChild(script);
    });
  };
  ws.onclose = function() { setTimeout(function() { location.reload() }, 1000) };
})();
