"""Decode a Path of Building export code (base64url + zlib) into build XML.

Usage: python pob_bridge/decode_pob_code.py CODE_FILE OUT.xml
"""
import base64
import sys
import zlib


def decode(code: str) -> str:
    code = code.strip()
    raw = base64.urlsafe_b64decode(code + "=" * (-len(code) % 4))
    return zlib.decompress(raw).decode("utf-8")


if __name__ == "__main__":
    src, dst = sys.argv[1], sys.argv[2]
    with open(src, encoding="utf-8") as f:
        xml = decode(f.read())
    with open(dst, "w", encoding="utf-8") as f:
        f.write(xml)
    print(f"wrote {dst} ({len(xml)} chars)")
