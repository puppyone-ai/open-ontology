"""Frame text, scene text and the pts timeline (10 section 17.5 steps 4 to 6)."""

KIND_ORDER = {"scene": 0, "frame_text": 1, "utterance": 2}


def norm_line(t):
    return "".join(t.split())


def merge_scene_text(line_lists):
    """Deduplicated union of the lines of all OCR'd frames of one scene, in first-seen order."""
    seen, out = set(), []
    for lines in line_lists:
        for l in lines:
            k = norm_line(l["text"])
            if k and k not in seen:
                seen.add(k)
                out.append(l["text"])
    return "\n".join(out)


def frame_record(t_ms, lines):
    """One `frame_text` item: text of its lines, bbox = union of the line boxes in frame pixels."""
    lines = [l for l in lines if l["text"].strip()]
    if not lines:
        return None
    bb = [min(l["bbox_px"][0] for l in lines), min(l["bbox_px"][1] for l in lines),
          max(l["bbox_px"][2] for l in lines), max(l["bbox_px"][3] for l in lines)]
    conf = sum(l.get("score", 1.0) for l in lines) / len(lines)
    return {"kind": "frame_text", "t_ms": t_ms, "text": "\n".join(l["text"] for l in lines),
            "bbox": [int(round(v)) for v in bb], "confidence": round(conf, 4)}


def build_timeline(keyframes, spans, frame_lines, utterances, offset_ms=0):
    """Merge scenes, frame texts and utterances into one list ordered by source-axis milliseconds.

    `keyframes`: [(t_ms, image, scene_index)] (the image is not used here); `spans`: scene (start, end) in the
    same time base; `frame_lines`: {scene_index: [line, ...]} OCR lines of that scene's key frame;
    `utterances` already carry source-axis times. `offset_ms` moves frame and scene times to the source axis.
    Ties: scene, then frame text, then utterance, then original order: the output is deterministic.
    """
    items = []
    for i, (s, e) in enumerate(spans):
        lines = frame_lines.get(i, [])
        text = merge_scene_text([lines])
        if text:
            items.append({"kind": "scene", "t_start_ms": s + offset_ms, "t_end_ms": e + offset_ms, "text": text})
        rec = frame_record(keyframes[i][0] + offset_ms, lines)
        if rec:
            items.append(rec)
    for u in utterances:
        items.append({"kind": "utterance", **u})

    def key(ix):
        j, it = ix
        t = it.get("t_start_ms", it.get("t_ms", 0))
        return (t, KIND_ORDER[it["kind"]], j)

    return [it for _, it in sorted(enumerate(items), key=key)]
