// Test-only HTTP server for the Niva WASM network tests.
//
// It MUST run as its own process: the test's XMLHttpRequest
// polyfill calls curl via spawnSync, which blocks the test
// process's event loop — a server in the same process could
// never answer while a fetch is in flight (deadlock, curl
// times out after 30s). Run: `node test-server.mjs`, read
// `READY <port>` from stdout, kill when done.
import http from 'node:http';

const server = http.createServer((req, res) => {
  const cors = { 'Access-Control-Allow-Origin': '*' };
  if (req.url === '/hello') {
    const body = 'مرحبا من الخادم';
    res.writeHead(200, { ...cors, 'Content-Type': 'text/plain; charset=utf-8', 'Content-Length': Buffer.byteLength(body) });
    res.end(body);
  } else if (req.url === '/chunked') {
    res.writeHead(200, { ...cors, 'Content-Type': 'text/plain' });
    res.write('chunk');
    res.end('ed-ok');
  } else if (req.url === '/redirect') {
    res.writeHead(302, { ...cors, Location: '/hello' });
    res.end();
  } else if (req.url === '/cross') {
    res.writeHead(302, { ...cors, Location: `http://localhost:${server.address().port}/hello` });
    res.end();
  } else if (req.url === '/big') {
    res.writeHead(200, { ...cors, 'Content-Type': 'text/plain', 'Content-Length': 6 * 1024 * 1024 });
    res.end('x'.repeat(6 * 1024 * 1024));
  } else if (req.url === '/status500') {
    res.writeHead(500, { ...cors });
    res.end();
  } else {
    res.writeHead(404, { ...cors });
    res.end();
  }
});
server.listen(0, '127.0.0.1', () => {
  console.log(`READY ${server.address().port}`);
});
