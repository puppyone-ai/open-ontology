"""Write held multi-table producer inputs. Does not call the loader or Docker."""

from __future__ import annotations

import c5_native_join as join


def main() -> None:
    index = join.emit_multi_table()
    print(f"entries {len(index['entries'])} elf_called {index['elf_called']} intake {index['intake_71_consumable']}")


if __name__ == "__main__":
    main()
