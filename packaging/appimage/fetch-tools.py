#!/usr/bin/env python3
"""Fetch only checksum-pinned upstream AppImage tools into a private directory."""
import argparse
import hashlib
import json
from pathlib import Path
import urllib.request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('destination', type=Path)
args = parser.parse_args()
lock = Path(__file__).with_name('tools.lock.json')
args.destination.mkdir(parents=True, exist_ok=True)
for record in json.loads(lock.read_text()):
    path = args.destination / record['url'].rsplit('/', 1)[1]
    data = path.read_bytes() if path.exists() else urllib.request.urlopen(record['url'], timeout=120).read()
    if hashlib.sha256(data).hexdigest() != record['sha256']:
        raise SystemExit('Checksum mismatch; upstream input changed. Do not run: ' + str(path))
    path.write_bytes(data)
    path.chmod(0o755)
(args.destination / 'sources.json').write_bytes(lock.read_bytes())
