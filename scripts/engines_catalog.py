#!/usr/bin/env python3
"""Adds engine archives built by scripts/build_engine.sh to catalog/catalog.json.

Usage: python3 scripts/engines_catalog.py [dist-engines]

Each <engine>-<version>-<key>.(tar.gz|zip) with its .sha256 becomes the asset for
<key>, downloaded from {base_url}/engines/<engine>/<version>/ (uploaded there by
scripts/publish_engines.sh). The version must match the catalog's engine version.
"""
import json
import os
import re
import sys
from collections import OrderedDict

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
dist = sys.argv[1] if len(sys.argv) > 1 else os.path.join(root, "dist-engines")
path = os.path.join(root, "catalog", "catalog.json")
catalog = json.load(open(path), object_pairs_hook=OrderedDict)

pattern = re.compile(r"^(sd-server|whisper-server)-(.+)-((?:macos|linux|windows)-[a-z0-9_]+-[a-z]+)\.(tar\.gz|zip)$")
changed = 0
for name in sorted(os.listdir(dist)):
    m = pattern.match(name)
    if not m:
        continue
    engine, version, key, _ = m.groups()
    entry = catalog["engines"].get(engine)
    if entry is None:
        sys.exit(f"{engine} is not in the catalog")
    if entry["version"] != version:
        sys.exit(f"{name}: version {version} != catalog {engine} version {entry['version']}")
    sha = open(os.path.join(dist, name + ".sha256")).read().split()[0]
    size_mb = round(os.path.getsize(os.path.join(dist, name)) / 1e6, 1)
    asset = OrderedDict(
        file=name,
        sha256=sha,
        size_mb=size_mb,
        url=f"{{base_url}}/engines/{engine}/{version}/{name}",
    )
    entry["assets"][key] = [asset]
    entry["assets"] = OrderedDict(sorted(entry["assets"].items()))
    changed += 1
    print(f"{engine} {key}: {sha}")

if not changed:
    sys.exit(f"no engine archives found in {dist}")
with open(path, "w") as f:
    json.dump(catalog, f, indent=2)
    f.write("\n")
print(f"updated {changed} assets in catalog/catalog.json")
