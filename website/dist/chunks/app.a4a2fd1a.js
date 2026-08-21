__r("app.a4a2fd1a.js", function(exports, require, module) {
  var currentPage = null;
  var currentChunk = "page-reference.a6097f80.js";

  exports.mount = async function(root) {
    var page = await __i(currentChunk);
    page.mount(root);
    currentPage = page;
  };

  window.__hmr_remount = function(chunkId, nextModule) {
    if (chunkId !== currentChunk) return;
    if (currentPage && currentPage.__cleanup) currentPage.__cleanup();
    nextModule.mount(document.getElementById("root"));
    currentPage = nextModule;
  };

  exports.__cleanup = function() {
    if (currentPage && currentPage.__cleanup) currentPage.__cleanup();
    currentPage = null;
  };
});
