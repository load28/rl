__r("store.2e7e9919.js", function(exports, require, module) {
  var H = __h;
  var saved = {};
  try { saved = JSON.parse(localStorage.getItem("rl-site") || "{}") } catch (error) {}

  exports.language = H.signal(saved.language === "ko" ? "ko" : "en");
  exports.topic = H.signal("overview");

  exports.setLanguage = function(language) {
    exports.language.set(language === "ko" ? "ko" : "en");
    try { localStorage.setItem("rl-site", JSON.stringify({ language: exports.language.get() })) } catch (error) {}
  };

  exports.setTopic = function(topic) {
    exports.topic.set(topic);
  };
});
