#!/usr/bin/env swift

import AppKit
import CoreText
import Foundation

let scriptURL = URL(fileURLWithPath: #filePath)
let root = scriptURL.deletingLastPathComponent().deletingLastPathComponent()
let sourceURL = root.appendingPathComponent("src/logo.rs")
let outputURL = root.appendingPathComponent("assets/martty-lockup.svg")

let source = try String(contentsOf: sourceURL, encoding: .utf8)
let blockPattern = #"pub const MARTTY: \[&str; 6\] = \[(.*?)\];"#
let blockRegex = try NSRegularExpression(
    pattern: blockPattern,
    options: [.dotMatchesLineSeparators]
)
let sourceRange = NSRange(source.startIndex..<source.endIndex, in: source)
guard
    let blockMatch = blockRegex.firstMatch(in: source, range: sourceRange),
    let blockRange = Range(blockMatch.range(at: 1), in: source)
else {
    fatalError("could not find MARTTY rows in \(sourceURL.path)")
}

let block = String(source[blockRange])
let rowRegex = try NSRegularExpression(pattern: #""([^"\n]*)""#)
let blockRangeNS = NSRange(block.startIndex..<block.endIndex, in: block)
let rows = rowRegex.matches(in: block, range: blockRangeNS).compactMap { match -> String? in
    guard let range = Range(match.range(at: 1), in: block) else { return nil }
    return String(block[range])
}
guard rows.count == 6 else {
    fatalError("expected 6 MARTTY rows, found \(rows.count)")
}

let logoFontSize: CGFloat = 34
let urlFontSize: CGFloat = 17
let logoFontName = NSFont(name: "SFMono-Bold", size: logoFontSize)?.fontName
    ?? NSFont.monospacedSystemFont(ofSize: logoFontSize, weight: .bold).fontName
let urlFontName = NSFont(name: "SFMono-Regular", size: urlFontSize)?.fontName
    ?? NSFont.monospacedSystemFont(ofSize: urlFontSize, weight: .regular).fontName
let logoFont = CTFontCreateWithName(logoFontName as CFString, logoFontSize, nil)
let urlFont = CTFontCreateWithName(urlFontName as CFString, urlFontSize, nil)

func glyphs(for text: String, font: CTFont) -> ([CGGlyph], [CGSize]) {
    let value = text as NSString
    var characters = [UniChar](repeating: 0, count: value.length)
    value.getCharacters(&characters, range: NSRange(location: 0, length: value.length))
    var glyphs = [CGGlyph](repeating: 0, count: value.length)
    guard CTFontGetGlyphsForCharacters(font, characters, &glyphs, value.length) else {
        fatalError("font \(CTFontCopyPostScriptName(font)) cannot render \(text)")
    }
    var advances = [CGSize](repeating: .zero, count: value.length)
    CTFontGetAdvancesForGlyphs(font, .horizontal, glyphs, &advances, value.length)
    return (glyphs, advances)
}

func fixedAdvance(font: CTFont) -> CGFloat {
    let (_, advances) = glyphs(for: "M", font: font)
    return advances[0].width
}

func number(_ value: CGFloat) -> String {
    String(format: "%.2f", Double(value))
}

func pathData(_ path: CGPath, offsetX: CGFloat, baseline: CGFloat) -> String {
    var commands: [String] = []
    path.applyWithBlock { pointer in
        let element = pointer.pointee
        func point(_ index: Int) -> CGPoint {
            let raw = element.points[index]
            return CGPoint(x: offsetX + raw.x, y: baseline - raw.y)
        }
        switch element.type {
        case .moveToPoint:
            let p = point(0)
            commands.append("M\(number(p.x)) \(number(p.y))")
        case .addLineToPoint:
            let p = point(0)
            commands.append("L\(number(p.x)) \(number(p.y))")
        case .addQuadCurveToPoint:
            let control = point(0)
            let end = point(1)
            commands.append(
                "Q\(number(control.x)) \(number(control.y)) \(number(end.x)) \(number(end.y))"
            )
        case .addCurveToPoint:
            let control1 = point(0)
            let control2 = point(1)
            let end = point(2)
            commands.append(
                "C\(number(control1.x)) \(number(control1.y)) "
                    + "\(number(control2.x)) \(number(control2.y)) "
                    + "\(number(end.x)) \(number(end.y))"
            )
        case .closeSubpath:
            commands.append("Z")
        @unknown default:
            fatalError("unsupported CGPath element")
        }
    }
    return commands.joined(separator: " ")
}

func outlines(
    text: String,
    font: CTFont,
    originX: CGFloat,
    baseline: CGFloat,
    cellAdvance: CGFloat
) -> String {
    let (glyphList, _) = glyphs(for: text, font: font)
    return glyphList.enumerated().compactMap { index, glyph -> String? in
        guard let path = CTFontCreatePathForGlyph(font, glyph, nil) else { return nil }
        return pathData(path, offsetX: originX + CGFloat(index) * cellAdvance, baseline: baseline)
    }.joined(separator: " ")
}

func interpolate(_ from: (Int, Int, Int), _ to: (Int, Int, Int), step: Int, total: Int) -> String {
    let amount = Double(step) / Double(max(total, 1))
    let values = [0, 1, 2].map { index -> Int in
        let start = [from.0, from.1, from.2][index]
        let end = [to.0, to.1, to.2][index]
        return Int((Double(start) + Double(end - start) * amount).rounded())
    }
    return String(format: "#%02x%02x%02x", values[0], values[1], values[2])
}

let cellAdvance = fixedAdvance(font: logoFont)
let lineHeight = ceil(CTFontGetAscent(logoFont) + CTFontGetDescent(logoFont) + CTFontGetLeading(logoFont))
let marginX: CGFloat = 44
let marginTop: CGFloat = 30
let split = 27
let artColumns = 54
let artWidth = CGFloat(artColumns) * cellAdvance
let artHeight = CGFloat(rows.count) * lineHeight
let url = "https://martty.sh"
let urlAdvance = fixedAdvance(font: urlFont)
let urlWidth = CGFloat(url.utf16.count) * urlAdvance
let urlGap: CGFloat = 30
let marginBottom: CGFloat = 30
let canvasWidth = ceil(artWidth + marginX * 2)
let canvasHeight = ceil(marginTop + artHeight + urlGap + CTFontGetAscent(urlFont) + marginBottom)

var paths: [String] = []
for (rowIndex, row) in rows.enumerated() {
    let units = Array(row.utf16)
    let mar = String(decoding: units.prefix(split), as: UTF16.self)
    let tty = String(decoding: units.dropFirst(split), as: UTF16.self)
    let baseline = marginTop + CTFontGetAscent(logoFont) + CGFloat(rowIndex) * lineHeight
    paths.append(
        "<path class=\"mar-\(rowIndex)\" d=\"\(outlines(text: mar, font: logoFont, originX: marginX, baseline: baseline, cellAdvance: cellAdvance))\"/>"
    )
    paths.append(
        "<path class=\"tty\" d=\"\(outlines(text: tty, font: logoFont, originX: marginX + CGFloat(split) * cellAdvance, baseline: baseline, cellAdvance: cellAdvance))\"/>"
    )
}

let urlBaseline = marginTop + artHeight + urlGap + CTFontGetAscent(urlFont)
let urlX = (canvasWidth - urlWidth) / 2
paths.append(
    "<path class=\"url\" d=\"\(outlines(text: url, font: urlFont, originX: urlX, baseline: urlBaseline, cellAdvance: urlAdvance))\"/>"
)

let lightOcean = rows.indices.map {
    interpolate((65, 118, 230), (211, 226, 255), step: $0, total: rows.count - 1)
}
let darkOcean = rows.indices.map {
    interpolate((237, 243, 254), (86, 134, 254), step: $0, total: rows.count - 1)
}
let lightOceanRules = lightOcean.enumerated().map { ".mar-\($0.offset) { fill: \($0.element); }" }
let darkOceanRules = darkOcean.enumerated().map { ".mar-\($0.offset) { fill: \($0.element); }" }
let cardWidth = Int(canvasWidth) - 2
let cardHeight = Int(canvasHeight) - 2

let svg = """
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 \(Int(canvasWidth)) \(Int(canvasHeight))" role="img" aria-labelledby="title desc">
  <title id="title">Martty terminal lockup</title>
  <desc id="desc">Generated from the MARTTY terminal-art source with CoreText glyph outlines.</desc>
  <style>
    .background { fill: #f5f8fc; stroke: #d6dfeb; stroke-width: 2; }
    .tty { fill: #26364f; }
    .url { fill: #6c7b93; }
    \(lightOceanRules.joined(separator: "\n    "))
    @media (prefers-color-scheme: dark) {
      .background { fill: #0f1015; stroke: #252b38; }
      .tty { fill: #f8f9fa; }
      .url { fill: #8f96a3; }
      \(darkOceanRules.joined(separator: "\n      "))
    }
  </style>
  <rect class="background" x="1" y="1" width="\(cardWidth)" height="\(cardHeight)" rx="20"/>
  \(paths.joined(separator: "\n  "))
</svg>
"""

try svg.write(to: outputURL, atomically: true, encoding: .utf8)
print("wrote \(outputURL.path) with \(logoFontName) outlines")
