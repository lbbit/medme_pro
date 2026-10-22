#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""负向证据辅助：把真服务的 /api/v1 与 /openapi.json 反代到本地，
   但把首页 `/` 换成**故意坏掉**的页面（用来证明浏览器门真的有判别力）。

用法：python3 tools/web_browser_broken_proxy.py <upstream_base> <listen_port>
"""
import http.server
import socketserver
import sys
import threading
import urllib.error
import urllib.request

UP = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:18699"
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 18698

BROKEN = """<!DOCTYPE html><html><head><meta charset="utf-8"><title>broken</title></head>
<body>
<header><span class="ver" id="ver">…</span></header>
<main><aside><input id="newName"><button id="btnAdd">建档</button><ul id="plist"></ul></aside>
<section><div class="tabs" id="tabs"></div><div id="pane"></div></section></main>
<script>
const API = location.origin + "/api/v1";
let persons = [], cur = null, tab = "timeline";
async function j(path, opt){ const r = await fetch(API+path, opt||{}); return await r.json(); }
// 故意坏掉：建档后不刷新侧栏（模拟「点了没反应」），且不渲染任何内容
document.getElementById("btnAdd").onclick = async () => {
  const name = document.getElementById("newName").value.trim(); if(!name) return;
  const p = await j("/persons", {method:"POST", headers:{"content-type":"application/json"}, body: JSON.stringify({name})});
  cur = p.id;
};
(async () => { try { persons = await j("/persons"); } catch(e){} })();
</script></body></html>"""


class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"  # 每请求关连接 ⇒ 不会因 keep-alive 死锁

    def log_message(self, *a):
        pass

    def do_GET(self):
        if self.path in ("/", "/index.html"):
            body = BROKEN.encode()
            self.send_response(200)
            self.send_header("content-type", "text/html; charset=utf-8")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        self._proxy("GET")

    def do_POST(self):
        self._proxy("POST")

    def _proxy(self, method):
        n = int(self.headers.get("content-length") or 0)
        data = self.rfile.read(n) if n else None
        req = urllib.request.Request(UP + self.path, data=data, method=method)
        for k, v in self.headers.items():
            if k.lower() not in ("host", "content-length"):
                req.add_header(k, v)
        try:
            r = urllib.request.urlopen(req)
            body = r.read()
            self.send_response(r.status)
            self.send_header("content-type", r.headers.get("content-type", "application/json"))
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except urllib.error.HTTPError as e:
            body = e.read()
            self.send_response(e.code)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)


socketserver.TCPServer.allow_reuse_address = True


class Server(socketserver.ThreadingTCPServer):
    daemon_threads = True
    allow_reuse_address = True


with Server(("127.0.0.1", PORT), H) as httpd:
    print("broken proxy on %d -> %s" % (PORT, UP), flush=True)
    httpd.serve_forever()
