from __future__ import annotations

from pathlib import Path
import sys


REQUIRED_PATHS = [
    "Cargo.toml",
    "README.md",
    "apps/core-service/Cargo.toml",
    "apps/core-service/src/main.rs",
    "crates/local-ipc/Cargo.toml",
    "crates/local-ipc/src/lib.rs",
    "apps/app-desktop/package.json",
    "apps/app-desktop/src-tauri/Cargo.toml",
    "apps/app-desktop/src-tauri/src/main.rs",
    "apps/app-desktop/src-tauri/tauri.conf.json",
    "apps/app-desktop/web/index.html",
]


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    missing = [rel for rel in REQUIRED_PATHS if not (root / rel).exists()]

    if missing:
        print("missing required paths:")
        for item in missing:
            print(f"- {item}")
        return 1

    print("structure check passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())

