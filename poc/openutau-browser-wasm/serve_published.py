#!/usr/bin/env python3
"""Serve the published POC with its precompressed .gz files."""

from __future__ import annotations

import argparse
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


class PrecompressedHandler(SimpleHTTPRequestHandler):
    def send_head(self):
        path = Path(self.translate_path(self.path))
        accepts_gzip = "gzip" in self.headers.get("Accept-Encoding", "")
        compressed_path = Path(f"{path}.gz")

        if path.is_file() and accepts_gzip and compressed_path.is_file():
            file = compressed_path.open("rb")
            stat = compressed_path.stat()
            self.send_response(200)
            self.send_header("Content-type", self.guess_type(str(path)))
            self.send_header("Content-Encoding", "gzip")
            self.send_header("Vary", "Accept-Encoding")
            self.send_header("Content-Length", str(stat.st_size))
            self.send_header("Last-Modified", self.date_time_string(stat.st_mtime))
            self.end_headers()
            return file

        return super().send_head()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=5201)
    parser.add_argument(
        "--directory",
        type=Path,
        default=Path(__file__).parent / "artifacts" / "publish" / "wwwroot",
    )
    args = parser.parse_args()
    directory = args.directory.resolve()
    handler = partial(PrecompressedHandler, directory=str(directory))
    server = ThreadingHTTPServer(("127.0.0.1", args.port), handler)
    print(f"Serving {directory} at http://127.0.0.1:{args.port}/")
    server.serve_forever()


if __name__ == "__main__":
    main()
