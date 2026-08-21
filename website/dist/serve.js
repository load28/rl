var http = require('http');
var fs = require('fs');
var path = require('path');
var WebSocket = require('ws');
var chokidar = require('chokidar');
var PORT = Number(process.env.PORT || 3000);
var DIR = __dirname;
var MIME = { '.html':'text/html','.js':'application/javascript','.css':'text/css','.json':'application/json','.svg':'image/svg+xml' };
var graph = {};

function node(id) {
  if (!graph[id]) graph[id] = { importers: new Set(), deps: new Set(), selfAccept: false };
  return graph[id];
}

function inspect(file) {
  var content = fs.readFileSync(path.join(DIR, 'chunks', file), 'utf8');
  var current = node(file);
  current.deps.forEach(function(dep) { node(dep).importers.delete(file) });
  current.deps = new Set();
  current.selfAccept = /exports\.mount\s*=/.test(content);
  var match;
  var imports = /require\(["']([^"']+)["']\)/g;
  while ((match = imports.exec(content)) !== null) {
    current.deps.add(match[1]);
    node(match[1]).importers.add(file);
  }
}

fs.readdirSync(path.join(DIR, 'chunks')).filter(function(file) { return file.endsWith('.js') }).forEach(inspect);

function boundaries(changed) {
  var found = [];
  function walk(id, seen) {
    var current = graph[id];
    if (!current) return true;
    if (current.selfAccept) { found.push(id); return false }
    if (!current.importers.size) return true;
    var reload = false;
    current.importers.forEach(function(importer) {
      if (seen.indexOf(importer) !== -1 || walk(importer, seen.concat(importer))) reload = true;
    });
    return reload;
  }
  return walk(changed, [changed]) ? null : found.filter(function(id, index, all) { return all.indexOf(id) === index });
}

var server = http.createServer(function(req, res) {
  var url = req.url.split('?')[0];
  var file = path.join(DIR, url);
  if (url !== '/' && fs.existsSync(file) && fs.statSync(file).isFile()) {
    res.writeHead(200, { 'Content-Type': MIME[path.extname(file)] || 'application/octet-stream', 'Cache-Control': 'no-cache' });
    fs.createReadStream(file).pipe(res);
    return;
  }
  res.writeHead(200, { 'Content-Type': 'text/html', 'Cache-Control': 'no-cache' });
  var html = fs.readFileSync(path.join(DIR, 'index.html'), 'utf8');
  res.end(html.replace('</body>', '<script src="hmr-client.js"></script>\n</body>'));
});

var wss = new WebSocket.Server({ server: server });
var clients = new Set();
wss.on('connection', function(ws) { clients.add(ws); ws.on('close', function() { clients.delete(ws) }) });
function broadcast(message) {
  var data = JSON.stringify(message);
  clients.forEach(function(ws) { if (ws.readyState === WebSocket.OPEN) ws.send(data) });
}

chokidar.watch(path.join(DIR, 'chunks'), { ignoreInitial: true }).on('change', function(filePath) {
  var file = path.basename(filePath);
  inspect(file);
  var accepted = boundaries(file);
  if (!accepted) { broadcast({ type: 'full-reload' }); return }
  var timestamp = Date.now();
  broadcast({ type: 'update', updates: accepted.map(function(id) { return { type: 'js-update', path: id, acceptedPath: file, timestamp: timestamp } }) });
});
chokidar.watch(path.join(DIR, 'index.html'), { ignoreInitial: true }).on('change', function() { broadcast({ type: 'full-reload' }) });
server.listen(PORT, function() { console.log('http://localhost:' + PORT) });
