// super-drive-media-decoder (20-frontend F-S10-05, D65): decodes media the engine's own FFmpeg cannot (H.264, HEVC, AAC, HEIC)
// with AVFoundation and writes what the engine reads from its exchange directory:
//   <output>/manifest.json  { protocol, helper_version, source_blob_sha256, audio, duration_ms, pts_ms, files{sha256: name} }
//   <output>/audio.wav      16 kHz mono 16-bit PCM
//   <output>/frames/%08d.png  one PNG per second of video (pts_ms lists the real times)
//   <output>/image.png      for a still image (HEIC, JPEG, PNG)
// Rules: no network, nothing written outside --output (a sibling temporary directory is renamed into place at the end, so a
// reader never sees a half-written result), progress as JSON lines on stderr, a SHA-256 of the source in the manifest.
// Exit codes: 0 done, 2 usage, 3 unreadable, 4 nothing decodable, 5 decoding failed, 6 the file changed (--expect-sha256).
import AVFoundation
import CryptoKit
import Foundation
import ImageIO
import UniformTypeIdentifiers

let helperVersion = "avf-1"
let sampleRate = 16_000
let exitUsage: Int32 = 2, exitUnreadable: Int32 = 3, exitNoTrack: Int32 = 4, exitDecode: Int32 = 5, exitChanged: Int32 = 6

/// The temporary output directory to remove when the helper stops early.
nonisolated(unsafe) var cleanupDirectory: URL?

func fail(_ code: Int32, _ message: String) -> Never {
    if let directory = cleanupDirectory { try? FileManager.default.removeItem(at: directory) }
    FileHandle.standardError.write(Data((#"{"error":"\#(message.replacingOccurrences(of: "\"", with: "'"))","code":\#(code)}"# + "\n").utf8))
    exit(code)
}

func progress(_ done: Int, _ total: Int, _ stage: String) {
    FileHandle.standardError.write(Data((#"{"progress":{"done_ms":\#(done),"total_ms":\#(total),"stage":"\#(stage)"}}"# + "\n").utf8))
}

func sha256(of url: URL) throws -> String {
    let handle = try FileHandle(forReadingFrom: url)
    defer { try? handle.close() }
    var hasher = SHA256()
    while let chunk = try handle.read(upToCount: 1 << 20), !chunk.isEmpty { hasher.update(data: chunk) }
    return hasher.finalize().map { String(format: "%02x", $0) }.joined()
}

func littleEndian<T: FixedWidthInteger>(_ value: T) -> [UInt8] {
    withUnsafeBytes(of: value.littleEndian) { Array($0) }
}

func wavHeader(dataBytes: Int) -> Data {
    var header = Data()
    header.append(contentsOf: Array("RIFF".utf8)); header.append(contentsOf: littleEndian(UInt32(36 + dataBytes)))
    header.append(contentsOf: Array("WAVEfmt ".utf8)); header.append(contentsOf: littleEndian(UInt32(16)))
    header.append(contentsOf: littleEndian(UInt16(1))); header.append(contentsOf: littleEndian(UInt16(1)))
    header.append(contentsOf: littleEndian(UInt32(sampleRate))); header.append(contentsOf: littleEndian(UInt32(sampleRate * 2)))
    header.append(contentsOf: littleEndian(UInt16(2))); header.append(contentsOf: littleEndian(UInt16(16)))
    header.append(contentsOf: Array("data".utf8)); header.append(contentsOf: littleEndian(UInt32(dataBytes)))
    return header
}

/// Decodes the first audio track to 16 kHz mono 16-bit PCM; returns the number of PCM bytes written.
func decodeAudio(asset: AVURLAsset, track: AVAssetTrack, to url: URL, totalMs: Int) async throws -> Int {
    let reader = try AVAssetReader(asset: asset)
    let settings: [String: Any] = [
        AVFormatIDKey: kAudioFormatLinearPCM, AVSampleRateKey: sampleRate, AVNumberOfChannelsKey: 1,
        AVLinearPCMBitDepthKey: 16, AVLinearPCMIsFloatKey: false, AVLinearPCMIsBigEndianKey: false, AVLinearPCMIsNonInterleaved: false,
    ]
    let output = AVAssetReaderTrackOutput(track: track, outputSettings: settings)
    guard reader.canAdd(output) else { fail(exitDecode, "cannot read the audio track") }
    reader.add(output)
    guard reader.startReading() else { fail(exitDecode, "cannot start reading audio") }
    FileManager.default.createFile(atPath: url.path, contents: nil)
    let handle = try FileHandle(forWritingTo: url)
    defer { try? handle.close() }
    try handle.write(contentsOf: wavHeader(dataBytes: 0))
    var written = 0
    var lastReport = 0
    while let buffer = output.copyNextSampleBuffer() {
        guard let block = CMSampleBufferGetDataBuffer(buffer) else { continue }
        let length = CMBlockBufferGetDataLength(block)
        var bytes = [UInt8](repeating: 0, count: length)
        CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: length, destination: &bytes)
        try handle.write(contentsOf: Data(bytes))
        written += length
        let doneMs = written / 2 * 1000 / sampleRate
        if doneMs - lastReport >= 5_000 { progress(min(doneMs, totalMs), totalMs, "audio"); lastReport = doneMs }
    }
    if reader.status == .failed { fail(exitDecode, "audio decoding failed: \(reader.error?.localizedDescription ?? "unknown")") }
    try handle.seek(toOffset: 0)
    try handle.write(contentsOf: wavHeader(dataBytes: written))
    return written
}

func writePNG(_ image: CGImage, to url: URL) throws {
    guard let destination = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil) else { fail(exitDecode, "cannot create a PNG") }
    CGImageDestinationAddImage(destination, image, nil)
    guard CGImageDestinationFinalize(destination) else { fail(exitDecode, "cannot write a PNG") }
}

/// One frame per second at the real presentation time; returns the pts (ms) of the frames written, in order.
func decodeFrames(asset: AVURLAsset, durationMs: Int, to directory: URL) throws -> [Int] {
    let generator = AVAssetImageGenerator(asset: asset)
    generator.appliesPreferredTrackTransform = true
    generator.requestedTimeToleranceBefore = .zero
    generator.requestedTimeToleranceAfter = .zero
    var pts: [Int] = []
    var second = 0
    while second * 1000 < max(durationMs, 1) {
        let requested = CMTime(value: CMTimeValue(second * 1000), timescale: 1000)
        var actual = CMTime.zero
        let image = try generator.copyCGImage(at: requested, actualTime: &actual)
        try writePNG(image, to: directory.appendingPathComponent(String(format: "%08d.png", pts.count + 1)))
        pts.append(Int((actual.seconds * 1000).rounded()))
        second += 1
        progress(min(second * 1000, durationMs), durationMs, "frames")
    }
    return pts
}

func decodeStillImage(_ input: URL, to url: URL) throws -> Bool {
    guard let source = CGImageSourceCreateWithURL(input as CFURL, nil), CGImageSourceGetCount(source) > 0,
          let image = CGImageSourceCreateImageAtIndex(source, 0, [kCGImageSourceShouldCache: false] as CFDictionary) else { return false }
    try writePNG(image, to: url)
    return true
}

func run() async {
    var input: String?, output: String?, expected: String?
    var index = 1
    let arguments = CommandLine.arguments
    while index < arguments.count {
        switch arguments[index] {
        case "--input": index += 1; input = index < arguments.count ? arguments[index] : nil
        case "--output": index += 1; output = index < arguments.count ? arguments[index] : nil
        case "--expect-sha256": index += 1; expected = index < arguments.count ? arguments[index] : nil
        case "--version": print(helperVersion); exit(0)
        default: fail(exitUsage, "unknown argument \(arguments[index])")
        }
        index += 1
    }
    guard let input, let output, input.hasPrefix("/"), output.hasPrefix("/") else { fail(exitUsage, "usage: --input <absolute file> --output <absolute directory>") }
    let source = URL(fileURLWithPath: input)
    let finalDirectory = URL(fileURLWithPath: output)
    let fileManager = FileManager.default
    guard fileManager.isReadableFile(atPath: source.path) else { fail(exitUnreadable, "the source file cannot be read") }
    let temporary = finalDirectory.deletingLastPathComponent().appendingPathComponent(".\(finalDirectory.lastPathComponent).tmp-\(getpid())")
    try? fileManager.removeItem(at: temporary)
    cleanupDirectory = temporary
    do {
        try fileManager.createDirectory(at: temporary, withIntermediateDirectories: true)
        let digest = try sha256(of: source)
        if let expected, expected != digest { fail(exitChanged, "the file changed since it was queued") }
        var manifest: [String: Any] = ["protocol": "oo.host-decode/v1", "helper_version": helperVersion, "source_blob_sha256": digest]
        var files: [String: String] = [:]
        let asset = AVURLAsset(url: source)
        let audioTracks = (try? await asset.loadTracks(withMediaType: .audio)) ?? []
        let videoTracks = (try? await asset.loadTracks(withMediaType: .video)) ?? []
        let duration = (try? await asset.load(.duration)) ?? .zero
        let durationMs = duration.isNumeric ? Int((duration.seconds * 1000).rounded()) : 0
        if audioTracks.isEmpty && videoTracks.isEmpty {
            // Not a time-based medium: a still image (HEIC and the like) becomes one PNG.
            let png = temporary.appendingPathComponent("image.png")
            guard try decodeStillImage(source, to: png) else { fail(exitNoTrack, "no decodable audio, video or image") }
            files[try sha256(of: png)] = "image.png"
            manifest["image"] = "image.png"
            manifest["duration_ms"] = 0
        } else {
            manifest["duration_ms"] = durationMs
            if let audio = audioTracks.first {
                let wav = temporary.appendingPathComponent("audio.wav")
                _ = try await decodeAudio(asset: asset, track: audio, to: wav, totalMs: durationMs)
                files[try sha256(of: wav)] = "audio.wav"
                manifest["audio"] = "audio.wav"
            }
            if !videoTracks.isEmpty {
                let frames = temporary.appendingPathComponent("frames")
                try fileManager.createDirectory(at: frames, withIntermediateDirectories: true)
                let pts = try decodeFrames(asset: asset, durationMs: durationMs, to: frames)
                for number in 1...max(pts.count, 1) where number <= pts.count {
                    let name = String(format: "%08d.png", number)
                    files[try sha256(of: frames.appendingPathComponent(name))] = "frames/\(name)"
                }
                manifest["pts_ms"] = pts
            }
        }
        manifest["files"] = files
        try JSONSerialization.data(withJSONObject: manifest, options: [.sortedKeys]).write(to: temporary.appendingPathComponent("manifest.json"))
        try? fileManager.removeItem(at: finalDirectory)
        try fileManager.moveItem(at: temporary, to: finalDirectory)
        print(String(data: try JSONSerialization.data(withJSONObject: manifest, options: [.sortedKeys]), encoding: .utf8) ?? "{}")
        exit(0)
    } catch {
        try? fileManager.removeItem(at: temporary)
        fail(exitDecode, "\(error.localizedDescription)")
    }
}

await run()
