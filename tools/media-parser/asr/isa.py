"""CPU instruction-set fingerprint written into `parser.profile.isa` (D71, D73)."""
import platform
import re

KEEP = ("sse4_2", "avx", "avx2", "avx512f", "fma", "neon", "asimd", "sve")


def isa_fingerprint(cpuinfo="/proc/cpuinfo"):
    flags = set()
    try:
        text = open(cpuinfo, encoding="utf-8", errors="replace").read()
        for m in re.finditer(r"^(?:flags|Features)\s*:\s*(.*)$", text, re.M):
            flags.update(m.group(1).split())
    except OSError:
        pass
    seen = sorted(f for f in flags if f in KEEP)
    return f"{platform.machine()}:{'+'.join(seen) or 'generic'}"
