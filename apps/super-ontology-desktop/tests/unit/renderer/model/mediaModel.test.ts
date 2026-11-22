import { describe, expect, it } from "vitest";
import { anchorSeconds, displayName, placeholderLetter, representativeClips, speakerLetter, type Utterance } from "../../../../src/renderer/model/details/mediaModel";

let n = 0;
const utt = (speaker: string, startS: number, endS: number, text = "words", lowConfidence = false): Utterance => ({ id: `u${n++}`, startMs: startS * 1000, endMs: endS * 1000, speaker, text, lowConfidence, src: null });

describe("speaker letters", () => {
  it("S1 is A, S26 is Z and S27 is AA, the way the engine names the placeholder", () => {
    expect([speakerLetter("S1"), speakerLetter("S2"), speakerLetter("S26"), speakerLetter("S27"), speakerLetter("S28")]).toEqual(["A", "B", "Z", "AA", "AB"]);
    expect(speakerLetter("host")).toBe("host");
  });
  it("reads a placeholder Person's letter and leaves a real name alone", () => {
    expect(placeholderLetter("Speaker A · 1a2b3c4d")).toBe("A");
    expect(placeholderLetter("Speaker AB · 1a2b3c4d")).toBe("AB");
    expect(placeholderLetter("Speaker 2 · 1a2b3c4d")).toBe("B");
    expect(placeholderLetter("说话人 S3 · 1a2b3c4d")).toBe("C");
    expect(placeholderLetter("Speaker Lee")).toBeNull();
    expect(placeholderLetter("林川")).toBeNull();
    expect(displayName("Speaker B · 0123abcd", (letter) => `说话人 ${letter}`)).toBe("说话人 B");
    expect(displayName("林川", (letter) => `说话人 ${letter}`)).toBe("林川");
  });
});

describe("clips that make a speaker recognisable", () => {
  it("takes the longest stretches, 5 to 10 seconds, in time order, and at most three", () => {
    const list = [utt("S1", 0, 3), utt("S2", 4, 20), utt("S1", 22, 29), utt("S2", 30, 31), utt("S1", 40, 52), utt("S1", 70, 76), utt("S1", 90, 93)];
    const clips = representativeClips(list, "S1");
    expect(clips.map((c) => [c.startMs / 1000, c.endMs / 1000])).toEqual([[22, 29], [40, 50], [70, 76]]);
    expect(clips.every((c) => c.speaker === "S1" && c.endMs - c.startMs <= 10_000)).toBe(true);
  });
  it("joins neighbouring utterances of one speaker and cuts a long one with its text in proportion", () => {
    const joined = representativeClips([utt("S1", 0, 3, "a"), utt("S1", 3.5, 8, "b")], "S1");
    expect(joined).toHaveLength(1);
    expect([joined[0].startMs, joined[0].endMs, joined[0].text]).toEqual([0, 8000, "a b"]);
    const long = representativeClips([utt("S1", 0, 20, "x".repeat(100))], "S1");
    expect([long[0].endMs, long[0].text.length]).toEqual([10_000, 50]);
  });
  it("puts low-confidence text last, copes with one short utterance or no speech", () => {
    const list = [utt("S1", 0, 9, "poor", true), utt("S1", 20, 26, "clean one"), utt("S2", 27, 28), utt("S1", 40, 46, "clean two")];
    expect(representativeClips(list, "S1", 2).map((c) => c.text)).toEqual(["clean one", "clean two"]);
    expect(representativeClips(list, "S1", 3).map((c) => c.text)).toEqual(["poor", "clean one", "clean two"]);
    expect(representativeClips([utt("S1", 0, 9, "a"), utt("S1", 12, 20, "b")], "S1", 3).map((c) => c.startMs)).toEqual([0, 12000]);
    expect(representativeClips([utt("S1", 0, 9, "a"), utt("S2", 9.2, 9.5), utt("S1", 12, 20, "b")], "S1", 3)).toHaveLength(2);
    expect(representativeClips([utt("S1", 0, 2)], "S1")).toHaveLength(1);
    expect(representativeClips([utt("S2", 0, 9)], "S1")).toEqual([]);
    expect(representativeClips([utt("S1", 0, 0.3)], "S1")).toEqual([]);
  });
});

describe("evidence anchors that are times", () => {
  it("reads the second a #t= mark starts at", () => {
    expect(anchorSeconds("t=00:12:31-00:12:48@S1")).toBe(751);
    expect(anchorSeconds("[src:a.m4a#t=00:00:05]")).toBe(5);
    expect(anchorSeconds("L6")).toBeNull();
  });
});
