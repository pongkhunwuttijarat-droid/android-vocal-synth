#!/usr/bin/env python3
"""List the import/export sections of a .wasm file (no external tools).

Used by web/run-moe.sh to prove the wasm module has NO filesystem/WASI imports:
every import must be a wasm-bindgen JS shim (__wbg_* / __wbindgen_*).

  python3 wasm_imports.py <file.wasm>   -> JSON on stdout
"""
import json, struct, sys

buf = open(sys.argv[1], "rb").read()
if buf[:4] != b"\x00asm":
    raise SystemExit("not a wasm file: %s" % sys.argv[1])
pos = 8

def uleb(p):
    r = 0; s = 0
    while True:
        b = buf[p]; p += 1
        r |= (b & 0x7f) << s
        if not (b & 0x80): return r, p
        s += 7

def name(p):
    n, p = uleb(p)
    return buf[p:p + n].decode("utf-8", "replace"), p + n

imports, exports = [], []
while pos < len(buf):
    sid = buf[pos]; pos += 1
    size, pos = uleb(pos)
    end = pos + size
    if sid == 2:
        cnt, p = uleb(pos)
        for _ in range(cnt):
            mod, p = name(p)
            nm, p = name(p)
            kind = buf[p]; p += 1
            if kind == 0:
                _, p = uleb(p)
            elif kind == 1:
                p += 1
                flags, p = uleb(p)
                if flags & 1: _, p = uleb(p)
                _, p = uleb(p)
                if flags & 2: _, p = uleb(p)
            elif kind == 2:
                flags, p = uleb(p)
                _, p = uleb(p)
                if flags & 1: _, p = uleb(p)
            elif kind == 3:
                p += 2
            imports.append({"module": mod, "name": nm})
    elif sid == 7:
        cnt, p = uleb(pos)
        for _ in range(cnt):
            nm, p = name(p)
            kind = buf[p]; p += 1
            _, p = uleb(p)
            exports.append(nm)
    pos = end

modules = sorted(set(i["module"] for i in imports))
print(json.dumps({
    "file": sys.argv[1],
    "bytes": len(buf),
    "importCount": len(imports),
    "modules": modules,
    "imports": imports,
    "exportCount": len(exports),
    "exports": exports,
}, ensure_ascii=False))
