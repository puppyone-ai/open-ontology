// Renders text/rect jobs (JSON on argv[1]) to PNG or raw 8-bit gray. Seeded noise only, no clocks.
import Foundation
import CoreGraphics
import CoreText
import ImageIO
import UniformTypeIdentifiers

struct Rng { var s: UInt64
  mutating func next() -> Double { s = s &* 6364136223846793005 &+ 1442695040888963407; return Double((s >> 33) & 0xFFFFFF) / Double(0x1000000) } }

func color(_ a: [Double]) -> CGColor { CGColor(red: a[0], green: a[1], blue: a[2], alpha: a.count > 3 ? a[3] : 1) }

func render(_ job: [String: Any]) {
  let w = job["w"] as! Int, h = job["h"] as! Int
  let out = job["out"] as! String
  let raw = (job["format"] as? String) == "gray"
  var rng = Rng(s: UInt64(job["seed"] as? Int ?? 1))
  let cs = CGColorSpaceCreateDeviceRGB()
  let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: cs,
                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
  ctx.setFillColor(color(job["bg"] as? [Double] ?? [1, 1, 1])); ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
  let rot = (job["rotate"] as? Double ?? 0) * Double.pi / 180
  ctx.saveGState()
  ctx.translateBy(x: CGFloat(w) / 2, y: CGFloat(h) / 2); ctx.rotate(by: CGFloat(rot)); ctx.translateBy(x: -CGFloat(w) / 2, y: -CGFloat(h) / 2)
  for op in job["ops"] as! [[String: Any]] {
    let kind = op["k"] as! String
    if kind == "rect" {
      let r = CGRect(x: op["x"] as! Double, y: Double(h) - (op["y"] as! Double) - (op["h"] as! Double), width: op["w"] as! Double, height: op["h"] as! Double)
      if let f = op["fill"] as? [Double] { ctx.setFillColor(color(f)); ctx.fill(r) }
      if let s = op["stroke"] as? [Double] { ctx.setStrokeColor(color(s)); ctx.setLineWidth(op["lw"] as? Double ?? 2); ctx.stroke(r) }
    } else if kind == "text" {
      let size = op["size"] as? Double ?? 24
      let font = CTFontCreateWithName((op["font"] as? String ?? "PingFangSC-Regular") as CFString, size, nil)
      let attrs: [NSAttributedString.Key: Any] = [NSAttributedString.Key(kCTFontAttributeName as String): font,
        NSAttributedString.Key(kCTForegroundColorAttributeName as String): color(op["color"] as? [Double] ?? [0, 0, 0])]
      let line = CTLineCreateWithAttributedString(NSAttributedString(string: op["text"] as! String, attributes: attrs))
      ctx.textPosition = CGPoint(x: op["x"] as! Double, y: Double(h) - (op["y"] as! Double))
      CTLineDraw(line, ctx)
    }
  }
  ctx.restoreGState()
  let noise = job["noise"] as? Double ?? 0
  if noise > 0 {
    let p = ctx.data!.assumingMemoryBound(to: UInt8.self); let bpr = ctx.bytesPerRow
    for y in 0..<h { for x in 0..<w {
      let d = Int((rng.next() - 0.5) * noise * 255)
      for c in 0..<3 { let i = y * bpr + x * 4 + c; p[i] = UInt8(max(0, min(255, Int(p[i]) + d))) } } }
  }
  if raw {
    let p = ctx.data!.assumingMemoryBound(to: UInt8.self); let bpr = ctx.bytesPerRow
    var g = [UInt8](repeating: 0, count: w * h)
    for y in 0..<h { for x in 0..<w { let i = y * bpr + x * 4
      g[y * w + x] = UInt8((Int(p[i]) * 30 + Int(p[i + 1]) * 59 + Int(p[i + 2]) * 11) / 100) } }
    try! Data(g).write(to: URL(fileURLWithPath: out))
  } else {
    let img = ctx.makeImage()!
    let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: out) as CFURL, UTType.png.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, img, nil); CGImageDestinationFinalize(dest)
  }
}

let data = try! Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
for j in try! JSONSerialization.jsonObject(with: data) as! [[String: Any]] { render(j) }
