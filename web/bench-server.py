#!/usr/bin/env python3
"""Serve web/ and capture a POSTed benchmark result (real wall-clock timing).

Headless Chromium's --virtual-time-budget freezes performance.now() during
synchronous tasks, so render timings measured that way read 0. This server
lets the page POST its measurements back over HTTP while running in real time.
"""
import http.server, socketserver, sys, threading, pathlib

# What gets served: `web/` by default, or the directory given as the first
# argument. The real-file test serves the repository root so the page can reach
# `test/golden/...` voicebanks and `.ustx` projects on the same origin.
ROOT = pathlib.Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else pathlib.Path(__file__).parent
# Binary POST target: the rendered WAV, so the runner can compare it with the
# native binary's output byte for byte.
WAV_PATH = pathlib.Path("/tmp/wasm-real.wav")
result = {}
done = threading.Event()

class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    """Threaded — the browser fetches hundreds of files concurrently and the
    stdlib single-threaded server serialises them."""
    daemon_threads = True
    allow_reuse_address = True


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=str(ROOT), **kw)

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(n) if n else b""
        path = self.path
        if path == "/__progress":
            # Stream progress lines so a hung step is visible.
            body = raw.decode("utf-8", "replace")
            with open("/tmp/dsh-progress.log", "a") as f:
                f.write(body + "\n")
            print(body, flush=True)
            self.send_response(204)
            self.end_headers()
            return
        if path == "/__wav":
            # Binary — never decoded.
            WAV_PATH.write_bytes(raw)
            print(f"WAV {WAV_PATH} {len(raw)} bytes", flush=True)
            self.send_response(204)
            self.end_headers()
            return
        result["body"] = raw.decode("utf-8", "replace")
        self.send_response(204)
        self.end_headers()
        done.set()

    def log_message(self, *a):
        pass

with Server(("127.0.0.1", 0), Handler) as httpd:
    port = httpd.server_address[1]
    print(f"PORT {port}", flush=True)
    t = threading.Thread(target=httpd.serve_forever, daemon=True)
    t.start()
    if done.wait(timeout=300):
        print("RESULT " + result.get("body", ""), flush=True)
    else:
        print("TIMEOUT", flush=True)
    httpd.shutdown()
