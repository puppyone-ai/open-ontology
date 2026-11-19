#!/usr/bin/env python3
"""Convert a STaRK semi-structured KB into the plain files the OO benchmark
driver reads: `nodes.jsonl` (one node per line: id, type, name, aliases,
doc) and `edges.tsv` (head, tail, relation).

Inputs are the official downloads (HF `snap-stanford/stark`, revision
88269e23e90587f99476c5dd74e235a0877e69be), `skb/<kb>/processed.zip`, plus
for Prime `skb/prime/raw.zip` extracted to `kg.csv` (its edges carry the
display relation names). Pickles are read with an unpickler that only
builds plain containers and scalars, and the `.pt` tensors with the same
unpickler plus a reader for the raw little-endian int64 storage, so no code
from the download ever runs and torch is not needed. Node text follows
`stark_qa.skb.<KB>SKB.get_doc_info` without relations.

Usage:
  python3 -I prepare_skb.py --kb prime --processed-zip P --kg-csv K --out DIR
  python3 -I prepare_skb.py --kb mag --processed-zip P --out DIR
  python3 -I prepare_skb.py --kb amazon --processed-zip P --out DIR
"""
import argparse
import array
import csv
import json
import pickle
import sys
import zipfile
from pathlib import Path

SAFE = {("builtins", n) for n in ("dict", "list", "set", "frozenset", "tuple", "str", "int", "float", "bool")}

GENE_EXPLAIN = {
    "name": "gene name",
    "type_of_gene": "gene types",
    "alias": "other gene names",
    "other_names": "extended other gene names",
    "genomic_pos": "genomic position",
    "generif": "PubMed text",
    "interpro": "protein family and classification information",
    "summary": "protein summary text",
}


class _DType:
    """Stand-in for `numpy.dtype`: keeps only the type code (`f8`, `i8`...)."""

    def __init__(self, code, *_):
        self.code = code
        self.order = "<"

    def __setstate__(self, state):
        if isinstance(state, tuple) and len(state) > 1 and state[1] in "<>|=":
            self.order = "<" if state[1] in "|=" else state[1]


_SCALAR = {"f8": "d", "f4": "f", "i8": "q", "i4": "i", "i2": "h", "i1": "b", "u8": "Q", "u4": "I", "u2": "H", "u1": "B", "b1": "?"}


def _scalar(dtype, data=b""):
    """Stand-in for `numpy.core.multiarray.scalar`: decodes plain numbers."""
    import struct

    if isinstance(data, str):
        data = data.encode("latin-1")
    if dtype.code.startswith("U"):
        return data.decode("utf-32-le" if dtype.order == "<" else "utf-32-be").rstrip("\x00")
    if dtype.code.startswith("S"):
        return data.rstrip(b"\x00").decode("utf-8", "replace")
    fmt = _SCALAR.get(dtype.code)
    if fmt is None:
        raise pickle.UnpicklingError(f"unsupported numpy scalar {dtype.code}")
    return struct.unpack(dtype.order + fmt, data)[0]


class PlainUnpickler(pickle.Unpickler):
    def find_class(self, module, name):
        if (module, name) in SAFE:
            return getattr(__import__(module), name)
        if (module, name) == ("collections", "OrderedDict"):
            return dict
        if (module, name) == ("numpy", "dtype"):
            return _DType
        if module in ("numpy.core.multiarray", "numpy._core.multiarray") and name == "scalar":
            return _scalar
        raise pickle.UnpicklingError(f"refusing {module}.{name}")


class _Storage:
    pass


def _rebuild_tensor(storage, offset, size, stride, *_):
    return {"storage": storage, "offset": offset, "size": tuple(size), "stride": tuple(stride)}


class TensorUnpickler(PlainUnpickler):
    """Reads `torch.save` metadata of an int64 tensor without torch."""

    def find_class(self, module, name):
        if (module, name) == ("torch._utils", "_rebuild_tensor_v2"):
            return _rebuild_tensor
        if (module, name) == ("torch", "LongStorage"):
            return _Storage
        return super().find_class(module, name)

    def persistent_load(self, pid):
        kind, cls, key, _location, numel = pid
        if kind != "storage" or cls is not _Storage:
            raise pickle.UnpicklingError("unexpected storage")
        return (key, numel)


def load_long_tensor(zf, member):
    """`member` is a `.pt` file inside the zip `zf`; returns rows of ints."""
    import io

    with zipfile.ZipFile(io.BytesIO(zf.read(member))) as pt:
        names = pt.namelist()
        meta_name = next(n for n in names if n.endswith("data.pkl"))
        prefix = meta_name[: -len("data.pkl")]
        meta = TensorUnpickler(io.BytesIO(pt.read(meta_name))).load()
        key, _numel = meta["storage"]
        buf = array.array("q")
        buf.frombytes(pt.read(prefix + "data/" + key))
    if sys.byteorder != "little":
        buf.byteswap()
    off, size, stride = meta["offset"], meta["size"], meta["stride"]
    if len(size) == 1:
        return buf[off : off + size[0] * stride[0] : stride[0]]
    assert len(size) == 2 and stride[1] == 1, "only row-major 2-D tensors"
    return [buf[off + r * stride[0] : off + r * stride[0] + size[1]] for r in range(size[0])]


def clean(d):
    out = {}
    for k, v in d.items():
        if isinstance(v, dict):
            out[k] = clean(v)
        elif str(v) not in ("", "nan"):
            out[k] = v
    return out


def doc_of(info):
    lines = [f"- name: {info['name']}", f"- type: {info['type']}", f"- source: {info['source']}"]
    feats = []
    for key, value in (info.get("details") or {}).items():
        if str(value) in ("", "nan") or key.startswith("_") or "_id" in key:
            continue
        if info["type"] == "gene/protein" and key in GENE_EXPLAIN:
            if "interpro" in key:
                value = [v.get("desc", "") for v in (value if isinstance(value, list) else [value]) if isinstance(v, dict)]
            if "generif" in key and isinstance(value, list):
                value = "; ".join(v.get("text", "") for v in value if isinstance(v, dict))
                value = " ".join(value.split(" ")[:50000])
            if "genomic_pos" in key and isinstance(value, list):
                value = value[0]
            feats.append(f"  - {key} ({GENE_EXPLAIN[key]}): {value}")
        else:
            feats.append(f"  - {key}: {value}")
    if feats:
        lines.append("- details:")
        lines.extend(feats)
    return "\n".join(lines)


def aliases_of(info):
    out = []
    det = info.get("details") or {}
    for key in ("alias", "other_names", "name", "mondo_name", "umls_description_name"):
        v = det.get(key)
        if isinstance(v, str):
            out.append(v)
        elif isinstance(v, list):
            out.extend(x for x in v if isinstance(x, str))
    seen, res = set(), []
    for a in out:
        a = a.strip()
        if a and a.lower() != str(info["name"]).lower() and a.lower() not in seen and len(a) <= 200:
            seen.add(a.lower())
            res.append(a)
    return res


def neg1(v):
    return str(v) in ("-1", "-1.0", "", "nan", "None")


def mag_doc(info):
    t = info["type"]
    if t == "paper":
        doc = f" - paper title: {info.get('title', '')}\n"
        doc += " - abstract: " + str(info.get("abstract", "")).replace("\r", "").rstrip("\n") + "\n"
        if not neg1(info.get("Date")):
            doc += f" - publication date: {info['Date']}\n"
        for key, label in (("OriginalVenue", "venue"), ("JournalDisplayName", "journal"),
                           ("ConferenceSeriesDisplayName", "conference"), ("ConferenceInstancesDisplayName", "conference")):
            if not neg1(info.get(key)):
                doc += f" - {label}: {info[key]}\n"
                break
        return str(info.get("title", "")), doc
    label = {"author": "author name", "field_of_study": "field of study", "institution": "institution"}[t]
    prefix = {"author": "author", "field_of_study": "field", "institution": "institution"}[t]
    doc = f"- {label}: {info.get('DisplayName')}\n"
    if not neg1(info.get("PaperCount")):
        doc += f"- {prefix} paper count: {info['PaperCount']}\n"
    if not neg1(info.get("CitationCount")):
        doc += f"- {prefix} citation count: {info['CitationCount']}\n"
    return str(info.get("DisplayName")), doc


def amazon_doc(info, ty, max_entries=25):
    """`stark_qa.skb.AmazonSKB.get_doc_info` without relations."""
    if ty == "brand":
        return str(info.get("brand_name")), f"brand name: {info.get('brand_name')}"
    if ty == "category":
        return str(info.get("category_name")), f"category name: {info.get('category_name')}"
    if ty == "color":
        return str(info.get("color_name")), f"color name: {info.get('color_name')}"
    title = str(info.get("title", ""))
    doc = f"- product: {title}\n"
    if "brand" in info:
        doc += f"- brand: {info['brand']}\n"
    dims = (info.get("details") or {}).get("product_dimensions") if isinstance(info.get("details"), dict) else None
    if isinstance(dims, str) and dims.count(" ; ") == 1:
        d, w = dims.split(" ; ")
        doc += f"- dimensions: {d}\n- weight: {w}\n"
    desc = info.get("description")
    if desc:
        desc = " ".join(desc if isinstance(desc, list) else [str(desc)]).strip(" ")
        if desc:
            doc += f"- description: {desc}\n"
    feats = info.get("feature") or []
    if feats:
        doc += "- features: \n"
        for i, f in enumerate(feats):
            if f and "asin" not in str(f).lower():
                doc += f"#{i + 1}: {f}\n"
    reviews = info.get("review") or []
    if reviews:
        doc += "- reviews: \n"

        def votes(r):
            v = r.get("vote")
            try:
                return int(str(v).replace(",", "")) if v is not None and str(v) != "nan" else 0
            except ValueError:
                return 0

        order = sorted(range(len(reviews)), key=lambda i: (-votes(reviews[i]), i))
        for k, i in enumerate(order):
            r = reviews[i]
            doc += f"#{i + 1}:\nsummary: {r.get('summary')}\ntext: \"{r.get('reviewText')}\"\n"
            if k > max_entries:
                break
    qas = info.get("qa") or []
    if qas:
        doc += "- Q&A: \n"
        for i, qa in enumerate(qas):
            doc += f"#{i + 1}:\nquestion: \"{qa.get('question')}\"\nanswer: \"{qa.get('answer')}\"\n"
            if i > max_entries:
                break
    return title, doc


def write_tensor_edges(zf, out, rel_names, prefix):
    ei = load_long_tensor(zf, prefix + "edge_index.pt")
    et = load_long_tensor(zf, prefix + "edge_types.pt")
    n = 0
    with open(out / "edges.tsv", "w") as w:
        for h, t, r in zip(ei[0], ei[1], et):
            w.write(f"{h}\t{t}\t{rel_names[r]}\n")
            n += 1
    return n


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--kb", choices=["prime", "mag", "amazon"], default="prime")
    ap.add_argument("--processed-zip", required=True)
    ap.add_argument("--kg-csv")
    ap.add_argument("--prefix", default="processed/",
                    help="folder inside the zip; Amazon: processed/cache/brand-category-color/ (the default AmazonSKB graph)")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    P = a.prefix
    with zipfile.ZipFile(a.processed_zip) as z:
        with z.open(P + "node_info.pkl") as f:
            node_info = PlainUnpickler(f).load()
        edge_types = PlainUnpickler(z.open(P + "edge_type_dict.pkl")).load()
        n = len(node_info)
        assert sorted(node_info) == list(range(n)), "node ids are not dense"
        if a.kb == "amazon":
            ntypes = load_long_tensor(z, P + "node_types.pt")
            ntype_names = PlainUnpickler(z.open(P + "node_type_dict.pkl")).load()
        with open(out / "nodes.jsonl", "w") as w:
            for i in range(n):
                info = node_info[i]
                if a.kb == "prime":
                    info = clean(info)
                    rec = {"id": i, "type": info["type"], "name": str(info["name"]), "aliases": aliases_of(info), "doc": doc_of(info)}
                elif a.kb == "mag":
                    name, doc = mag_doc(info)
                    rec = {"id": i, "type": info["type"], "name": name, "aliases": [], "doc": doc}
                else:
                    ty = ntype_names[int(ntypes[i])]
                    name, doc = amazon_doc(info, ty)[:2]
                    rec = {"id": i, "type": ty, "name": name, "aliases": [], "doc": doc}
                w.write(json.dumps(rec, ensure_ascii=False) + "\n")
        if a.kb == "prime":
            edges = 0
            with open(a.kg_csv, newline="") as r, open(out / "edges.tsv", "w") as w:
                for row in csv.DictReader(r):
                    w.write(f"{row['x_index']}\t{row['y_index']}\t{row['display_relation']}\n")
                    edges += 1
        else:
            names = {k: v.replace("___", " ") for k, v in edge_types.items()}
            edges = write_tensor_edges(z, out, names, P)
    print(json.dumps({"kb": a.kb, "nodes": n, "edges": edges}), file=sys.stderr)


if __name__ == "__main__":
    main()
