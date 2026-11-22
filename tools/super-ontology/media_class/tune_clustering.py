#!/usr/bin/env python3
"""Offline comparison of speaker count estimators on the window embeddings of `dump_emb.py` (lane AL). Run inside the media-parser image.

    python3 tune_clustering.py DIR [DIR...]
"""
import itertools
import sys
from pathlib import Path

sys.path.insert(0, "/opt/app")
import numpy as np  # noqa: E402
from asr import diarize as dz  # noqa: E402


def load(d):
    out = {}
    for f in sorted(Path(d).glob("*.npz")):
        z = np.load(f)
        out[f.stem] = (z["vecs"], z["spans"], z["ref"])
    return out


def wder(lab, ref):
    ok = ref >= 0
    lab, ref = np.asarray(lab)[ok], ref[ok]
    rl, hl = sorted(set(ref)), sorted(set(lab))
    best = len(lab)
    for perm in set(itertools.permutations(hl + [None] * len(rl), len(rl))):
        m = dict(zip(rl, perm))
        best = min(best, sum(1 for a, b in zip(ref, lab) if m.get(a) != b))
    return best / max(len(lab), 1)


def silhouette(V, lab):
    Vn = V / np.maximum(np.linalg.norm(V, axis=1, keepdims=True), 1e-9)
    D = 1 - Vn @ Vn.T
    s = []
    for i in range(len(V)):
        same = (lab == lab[i]); same[i] = False
        if not same.any():
            s.append(0.0); continue
        a = D[i, same].mean()
        b = min(D[i, lab == c].mean() for c in set(lab) if c != lab[i])
        s.append((b - a) / max(a, b, 1e-9))
    return float(np.mean(s))


def by_threshold(t):
    return lambda V, spans: dz.cluster(V, t)


def centered(fn):
    """The same estimator on embeddings minus their mean over the file: what all voices share (channel, synthesis) drops out."""
    return lambda V, spans: fn(V - V.mean(axis=0, keepdims=True), spans)


def by_eigengap(sigma=0.5, kmax=5, min_gap=0.0):
    """Spectral count: eigengap of the normalised Laplacian of a cosine affinity, then the agglomerative clustering at that count."""
    def f(V, spans):
        n = len(V)
        if n < 6:
            return np.zeros(n, dtype=int)
        Vn = V / np.maximum(np.linalg.norm(V, axis=1, keepdims=True), 1e-9)
        A = np.exp(-(1 - Vn @ Vn.T) / sigma)
        np.fill_diagonal(A, 0)
        d = A.sum(1)
        L = np.eye(n) - A / np.sqrt(np.outer(d, d))
        w = np.sort(np.linalg.eigvalsh(L))[: kmax + 1]
        gaps = np.diff(w)
        k = int(np.argmax(gaps[:kmax])) + 1
        if gaps[k - 1] < min_gap:
            k = 1
        return np.zeros(n, dtype=int) if k == 1 else dz.cluster(V, 9.9, num_speakers=k)
    return f


def by_eigen_sil(sigma=0.25, kmax=5, min_share=0.12, margin=0.0):
    """Eigengap proposes a count; the silhouette over counts k-1, k, k+1 (clusters of at least `min_share` of the windows) disposes."""
    base = by_eigengap(sigma, kmax)
    def f(V, spans):
        n = len(V)
        k0 = len(set(base(V, spans)))
        best, best_s = None, -9
        for k in {max(1, k0 - 1), k0, k0 + 1}:
            if k == 1:
                lab, sc = np.zeros(n, dtype=int), 0.0
            else:
                lab = dz.cluster(V, 9.9, num_speakers=k)
                if min(np.bincount(lab)) < min_share * n:
                    continue
                sc = silhouette(V, lab) - (margin if k != k0 else 0)
            if sc > best_s:
                best, best_s = lab, sc
        return best if best is not None else np.zeros(n, dtype=int)
    return f


def merge_small(fn, share, cap=10**9):
    """Clusters smaller than `share` of the windows are a clustering stray, not a speaker: they join the nearest cluster."""
    def f(V, spans):
        lab = np.asarray(fn(V, spans)).copy()
        n = len(lab)
        while True:
            counts = {c: int((lab == c).sum()) for c in set(lab)}
            small = [c for c, m in counts.items() if m < share * n and m <= cap]
            if not small or len(counts) == 1:
                return lab
            c = min(small, key=lambda x: counts[x])
            Vn = V / np.maximum(np.linalg.norm(V, axis=1, keepdims=True), 1e-9)
            cent = {k: Vn[lab == k].mean(0) for k in counts if k != c}
            mine = Vn[lab == c].mean(0)
            near = max(cent, key=lambda k: float(mine @ cent[k]) / (np.linalg.norm(mine) * np.linalg.norm(cent[k]) + 1e-9))
            lab[lab == c] = near
    return f


def by_silhouette(min_sil, kmax=5, min_share=0.08):
    def f(V, spans):
        if len(V) < 6:
            return np.zeros(len(V), dtype=int)
        best, best_s = np.zeros(len(V), dtype=int), min_sil
        for k in range(2, min(kmax, len(V) // 3) + 1):
            lab = dz.cluster(V, 9.9, num_speakers=k)
            if min(np.bincount(lab)) < min_share * len(V):
                continue
            s = silhouette(V, lab)
            if s > best_s + 1e-9:
                best, best_s = lab, s
        return best
    return f


def evaluate(name, fn, sets):
    rows = []
    for stem, (V, spans, ref) in sets.items():
        lab = np.asarray(fn(V, spans))
        truth = len(set(ref[ref >= 0]))
        rows.append((stem, truth, len(set(lab)), wder(lab, ref)))
    ok = sum(1 for _, t, h, _ in rows if t == h)
    return ok, len(rows), sum(r[3] for r in rows) / len(rows), rows


def main():
    sets = {}
    for d in sys.argv[1:]:
        sets.update(load(d))
    cands = {f"threshold {t:.2f}": by_threshold(t) for t in (0.55, 0.50, 0.45, 0.40, 0.35, 0.30)}
    for t in (1.5, 1.4, 1.3, 1.2, 1.1, 1.0, 0.95):
        cands[f"centered threshold {t:.2f}"] = centered(by_threshold(t))
    for sg in (0.10, 0.15, 0.20, 0.25, 0.30, 0.35, 0.40):
        for mg in (0.0,):
            cands[f"eigengap s={sg} min={mg}"] = by_eigengap(sg, 5, mg)
            cands[f"centered eigengap s={sg} min={mg}"] = centered(by_eigengap(sg, 5, mg))
    for cap in (2, 3, 4, 6, 10**9):
        cands[f"centered eigengap+merge 0.15 cap {cap}"] = centered(merge_small(by_eigengap(0.25, 5), 0.15, cap))
        cands[f"centered eigengap+merge 0.20 cap {cap}"] = centered(merge_small(by_eigengap(0.25, 5), 0.20, cap))
    for ms in (0.08,):
        cands[f"centered eigen+sil share={ms}"] = centered(by_eigen_sil(0.25, 5, ms))
    for s in (0.20,):
        cands[f"centered silhouette>={s:.2f}"] = centered(by_silhouette(s))
    import os
    if os.environ.get("DETAIL"):
        for name in os.environ["DETAIL"].split(","):
            print(name); detail(name, cands[name], sets)
        return
    for name, fn in cands.items():
        ok, n, der, _ = evaluate(name, fn, sets)
        print(f"{name:20} count {ok}/{n}  window-DER {der:.1%}", flush=True)


def detail(name, fn, sets):
    _, _, _, rows = evaluate(name, fn, sets)
    for stem, t, h, d in rows:
        print(f"  {stem:10} truth {t} found {h} wDER {d:.0%}")


if __name__ == "__main__":
    main()
