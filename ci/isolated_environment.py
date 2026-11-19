from __future__ import annotations

import os
from pathlib import Path
from typing import Mapping


def isolated_environment(
    root: str | Path, source: Mapping[str, str] | None = None
) -> dict[str, str]:

    root = Path(root).resolve()
    root.mkdir(parents=True, exist_ok=True)
    source = os.environ if source is None else source
    windows_keys = {"systemroot": "SystemRoot", "windir": "WINDIR"}
    environment = {
        windows_keys[key.lower()]: value
        for key, value in source.items()
        if key.lower() in windows_keys
    }
    directories = {
        "HOME": "home",
        "USERPROFILE": "home",
        "XDG_CONFIG_HOME": "config",
        "XDG_DATA_HOME": "data",
        "XDG_CACHE_HOME": "cache",
        "APPDATA": "data",
        "LOCALAPPDATA": "data",
        "TMPDIR": "tmp",
        "TEMP": "tmp",
        "TMP": "tmp",
        "PATH": "empty-path",
    }
    for key, relative in directories.items():
        path = root / relative
        path.mkdir(parents=True, exist_ok=True)
        environment[key] = str(path)
    environment["TZ"] = "UTC"
    return environment
