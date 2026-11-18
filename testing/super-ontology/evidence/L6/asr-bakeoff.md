# ASR and speaker-separation bake-off (D72, board B-S10-06)

Tool: `python3 -B tools/super-ontology/asr_bakeoff.py --golden testing/super-ontology/golden/media --tts say ...`
(`tools/super-ontology/test_asr_bakeoff.py` covers the metrics). Host: Apple silicon, one CPU thread per candidate, CPU only.

## Data

A synthetic 3-person Chinese project meeting, 17 turns, 96 s (`golden/media/script.json`, timing truth in
`golden/media/meeting-3spk.truth.json`). All names, projects and numbers are invented. Nobody talks over anyone; the
score is the exclusive-speaker one of 10 §17.3.

TTS: **no admissible open TTS could be fetched on this host.** The network blocked GitHub and Hugging Face for most of the run, so the
Apache-2.0 Kokoro model of sherpa-onnx (the planned generator, `--tts <kokoro dir>` is implemented) was not used. The audio was made with the
macOS `say` voices (Tingting, Sandy, Rocko; chosen by CAM++ cosine so that the three are separable but not trivially so: 0.46 between the
closest pair against 0.73 inside a voice). The WAV is generated at bake-off time, kept in the work directory, and **not committed or shipped**
(Apple's voices are not ours to redistribute); only the script, the generator and the timing truth are committed. Synthetic speech is
cleaner than a real meeting: read the CER as a ranking, not as a field accuracy.

## ASR

| Candidate | CER | CPU RTF | Peak RAM (MB) | License | Notes |
|---|---|---|---|---|---|
| Qwen3-ASR-0.6B, float32 | **0.0674** | 0.174 | 6016 | Apache-2.0 weights | over the 4 GiB media-parser cap |
| Qwen3-ASR-0.6B, bfloat16 | **0.0674** | 1.652 | 2580 | same | fits 4 GiB, 10x slower: 2 h of audio = 3.3 h |
| Paraformer-zh (FunASR), torch, fsmn-vad | 0.1348 | 0.029 | 2325 | Apache-2.0 (FunASR MIT) | needs torch |
| Paraformer-zh, quantized ONNX, chunks <= 12 s | 0.2163 | 0.040 | 573 | Apache-2.0 (own front end) | **no torch, fits 4 GiB** |
| Paraformer-zh, quantized ONNX, chunks <= 25 s | 0.3455 | 0.041 | 590 | same | accuracy falls on long inputs |
| FireRedASR2-AED | not run | | | Apache-2.0 | `model.pth.tar` is incomplete on the mirror and the FireRedASR code (GitHub) was unreachable |

Qwen3-ASR notes: the stock `transformers` 5.23 cannot load the released checkpoint (key layout and config differ: `thinker.*` weights,
`lm_head` shape mismatch), so the numbers come from the `qwen-asr` package in a **throw-away venv, measurement only**. That package
depends on GPL-3.0 soynlp and must not enter an image (D64); a shipped Qwen3-ASR needs our own loader. The Qwen3 forced aligner
(timestamps) was not run: disk was short. The Qwen3 word timestamps and hot words are therefore not measured.

## Speaker separation (own window clustering over 3D-Speaker CAM++ embeddings, energy VAD, 2 s windows)

| Cosine-distance stop | Speakers found | DER |
|---|---|---|
| **0.55** | **3** | **0.182** |
| 0.45 | 5 | 0.191 |
| 0.40 | 7 | 0.199 |
| 0.35 | 10 | 0.216 |
| 0.30 | 13 | 0.340 |
| 0.25 | 24 | 0.490 |

CPU RTF 0.02, peak RAM 560 MB (PyTorch path; the ONNX path of the image gives the same embeddings, cosine >= 0.9999). 0.55 finds the three speakers
and is the pipeline default. Hint `num_speakers` (meeting metadata) overrides the threshold.
pyannote community-1 was **not run**: it is gated (organisation, purpose and a marketing-mail agreement on Hugging Face) and this work does
not create accounts; the 90 §2.2 verdict (comparison only, never in the default image) is unchanged. DiariZen (CC-BY-NC) and SenseVoiceSmall
(license conflict) are excluded by D64 and were not run.

## End to end on the shipped engines (media-parser pipeline: silero VAD + Paraformer ONNX + CAM++ ONNX, 96 s)

CER **0.174**, DER **0.138**, 3 speakers (S1 to S3), 17 utterances, wall time 3.9 s (RTF 0.04). Timestamps are approximate at word level (the ONNX
export has no timestamp output; tokens are spread over voiced time).

## Conclusion (proposed for sign-off, acceptance 2)

* **Default for the `cpu` tier: Paraformer-zh quantized ONNX + 3D-Speaker CAM++ + silero VAD.** It is the only combination that fits 4 GiB
  (about 0.6 GB for the recognizer), runs at RTF 0.04 (a 2 h meeting in about 5 minutes), has no torch in the image and every part is
  default-usable in 90 §2. Cost: CER 0.17 to 0.22 on this set, no hot-word biasing (the plain model ignores them).
* **Quality upgrade: Qwen3-ASR-0.6B** (CER 0.067, half the error). Not shipped now: float32 needs 6 GB and bfloat16 is 10x slower on CPU;
  it also needs our own checkpoint loader (the `qwen-asr` package is forbidden). Recommended for the `host-accel` and `gpu` tiers (D71) once the loader
  and a bf16 or int8 memory fit exist, and for the forced aligner timestamps.
* Not evaluated and open: FireRedASR2-AED (blocked source), pyannote (gated), hot-word gain, real recordings.

License verdicts (90 §2.2): Qwen3-ASR weights default usable; `qwen-asr` package forbidden; FunASR code MIT and paraformer-zh,
fsmn-vad, CAM++ Apache-2.0 default usable (keep model names); sherpa-onnx and 3D-Speaker Apache-2.0 (we ship the 3D-Speaker model
as our own ONNX export, no sherpa-onnx in the image); silero-vad MIT by its LICENSE file (README badge disagrees, snapshot kept); pyannote and
FireRedASR2 LLM/Punc parts not enabled. Audit record text for `supply-chain/`: see the report.
