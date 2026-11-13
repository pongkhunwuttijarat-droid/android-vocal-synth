#!/usr/bin/env python3
"""Independent parameter count of a burn NamedMpk checkpoint.

Decodes the MessagePack container (no burn, no cargo) and sums every tensor shape
under the record 'item' map. Used by web/run-moe.sh to check that the wasm
instance really materialised that many parameter elements.

  python3 mpk_param_count.py <file.mpk>   -> JSON on stdout
"""
import json, struct, sys

buf = open(sys.argv[1], "rb").read()
pos = 0

def rd(n):
    global pos
    b = buf[pos:pos + n]; pos += n
    return b

def dec():
    global pos
    c = buf[pos]; pos += 1
    if c <= 0x7f: return c
    if c >= 0xe0: return c - 256
    if 0x80 <= c <= 0x8f: return {dec(): dec() for _ in range(c & 0x0f)}
    if 0x90 <= c <= 0x9f: return [dec() for _ in range(c & 0x0f)]
    if 0xa0 <= c <= 0xbf: return rd(c & 0x1f).decode("utf-8", "replace")
    if c == 0xc0: return None
    if c == 0xc2: return False
    if c == 0xc3: return True
    if c == 0xc4:
        n = rd(1)[0]; rd(n); return ("BIN", n)
    if c == 0xc5:
        n = struct.unpack(">H", rd(2))[0]; rd(n); return ("BIN", n)
    if c == 0xc6:
        n = struct.unpack(">I", rd(4))[0]; rd(n); return ("BIN", n)
    if c == 0xca: return struct.unpack(">f", rd(4))[0]
    if c == 0xcb: return struct.unpack(">d", rd(8))[0]
    if c == 0xcc: return rd(1)[0]
    if c == 0xcd: return struct.unpack(">H", rd(2))[0]
    if c == 0xce: return struct.unpack(">I", rd(4))[0]
    if c == 0xcf: return struct.unpack(">Q", rd(8))[0]
    if c == 0xd0: return struct.unpack(">b", rd(1))[0]
    if c == 0xd1: return struct.unpack(">h", rd(2))[0]
    if c == 0xd2: return struct.unpack(">i", rd(4))[0]
    if c == 0xd3: return struct.unpack(">q", rd(8))[0]
    if c == 0xd9: return rd(rd(1)[0]).decode("utf-8", "replace")
    if c == 0xda: return rd(struct.unpack(">H", rd(2))[0]).decode("utf-8", "replace")
    if c == 0xdb: return rd(struct.unpack(">I", rd(4))[0]).decode("utf-8", "replace")
    if c == 0xdc: return [dec() for _ in range(struct.unpack(">H", rd(2))[0])]
    if c == 0xdd: return [dec() for _ in range(struct.unpack(">I", rd(4))[0])]
    if c == 0xde: return {dec(): dec() for _ in range(struct.unpack(">H", rd(2))[0])}
    if c == 0xdf: return {dec(): dec() for _ in range(struct.unpack(">I", rd(4))[0])}
    raise SystemExit("unsupported byte 0x%02x at %d" % (c, pos - 1))

top = dec()
if pos != len(buf):
    raise SystemExit("decoder did not consume the whole file (%d / %d)" % (pos, len(buf)))
item = top.get("item") if isinstance(top, dict) else None
if item is None:
    raise SystemExit("no 'item' map in %s" % sys.argv[1])

tensors = []

def walk(node, path):
    if isinstance(node, dict):
        if "shape" in node and "dtype" in node:
            n = 1
            for s in node["shape"]:
                n *= s
            tensors.append({"key": path, "shape": node["shape"], "dtype": node["dtype"], "elems": n})
        else:
            for k, v in node.items():
                walk(v, path + "." + k if path else k)
    elif isinstance(node, list):
        for i, v in enumerate(node):
            walk(v, "%s[%d]" % (path, i))

walk(item, "")
meta = top.get("metadata", {})
print(json.dumps({
    "file": sys.argv[1],
    "bytes": len(buf),
    "tensors": len(tensors),
    "params": sum(t["elems"] for t in tensors),
    "dtypes": sorted(set(t["dtype"] for t in tensors)),
    "keys": [t["key"] for t in tensors],
    "recordFormat": meta.get("format"),
    "recordVersion": meta.get("version"),
}, ensure_ascii=False))
