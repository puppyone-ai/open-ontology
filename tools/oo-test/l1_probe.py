"""One-token model health probe through the repo's Egress (secret_ref only; never reads/prints values)."""
import importlib.util, json, sys, time
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("egress", ROOT / "tools/model-egress/model_egress.py")
eg = importlib.util.module_from_spec(spec); spec.loader.exec_module(eg)
MODEL = {"provider": "openai-compatible", "base_url": {"secret_ref": "model-endpoint-9r"},
         "key": {"secret_ref": "model-key-9r-sol"}, "model": "gpt-6.1-sol"}

def once(egress):
    started = time.monotonic()
    try:
        frames = list(egress.call({"model": MODEL["model"], "messages": [{"role": "user", "content": "."}],
                                   "max_tokens": 1}))
        ok = len(frames) >= 1 and isinstance(frames[0], dict) and isinstance(frames[0].get("choices"), list)
        return {"ok": ok, "code": "OK" if ok else "PROTOCOL", "seconds": round(time.monotonic() - started, 1)}
    except Exception as exc:
        code = getattr(exc, "code", type(exc).__name__)
        code = code if isinstance(code, str) and code.isupper() else type(exc).__name__
        return {"ok": False, "code": code, "seconds": round(time.monotonic() - started, 1)}

def healthy(log_path=None, need=3, budget=1800, gap=5):
    egress = eg.Egress(MODEL, None, rpm=60, timeout=60)
    attempts, streak, deadline = [], 0, time.monotonic() + budget
    try:
        while time.monotonic() < deadline:
            r = once(egress); r["at"] = time.strftime("%Y-%m-%dT%H:%M:%S"); attempts.append(r)
            streak = streak + 1 if r["ok"] else 0
            if streak >= need:
                break
            time.sleep(gap if r["ok"] else 30)
    finally:
        egress.close()
    result = {"healthy": streak >= need, "consecutive_required": need, "attempts": attempts}
    if log_path:
        Path(log_path).write_text(json.dumps(result, indent=2) + "\n")
    return result

if __name__ == "__main__":
    r = healthy(sys.argv[1] if len(sys.argv) > 1 else None)
    print(json.dumps({"healthy": r["healthy"], "attempts": len(r["attempts"]),
                      "codes": [a["code"] for a in r["attempts"]]}))
