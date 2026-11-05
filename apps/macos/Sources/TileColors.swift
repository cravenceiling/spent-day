import Foundation
import AppKit

struct TileRGB: Equatable, Sendable {
    var r: Double
    var g: Double
    var b: Double
    static let black = TileRGB(r: 0, g: 0, b: 0)
    static let white = TileRGB(r: 1, g: 1, b: 1)

    init(r: Double, g: Double, b: Double) { self.r = r; self.g = g; self.b = b }
    init(hex: UInt32) {
        r = Double((hex >> 16) & 255) / 255
        g = Double((hex >> 8) & 255) / 255
        b = Double(hex & 255) / 255
    }
    static func linear(_ v: Double) -> Double { v <= 0.04045 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4) }
    var luminance: Double { 0.2126 * Self.linear(r) + 0.7152 * Self.linear(g) + 0.0722 * Self.linear(b) }
    func contrast(_ other: Self) -> Double {
        (max(luminance, other.luminance) + 0.05) / (min(luminance, other.luminance) + 0.05)
    }
    var ink: Self { contrast(.black) >= contrast(.white) ? .black : .white }
    func dimmed(dark: Bool) -> Self {
        let background = dark ? Self(hex: 0x161618) : Self(hex: 0xf7f7f9)
        for step in stride(from: 20, through: 0, by: -1) {
            let t = Double(step) * 0.02
            let color = Self(r: r + (background.r - r) * t, g: g + (background.g - g) * t, b: b + (background.b - b) * t)
            if color.contrast(ink) >= 4.5 { return color }
        }
        return self
    }
    var lch: TileLCH {
        let r = Self.linear(r), g = Self.linear(g), b = Self.linear(b)
        let l = cbrt(0.4122214708*r + 0.5363015264*g + 0.0514459929*b)
        let m = cbrt(0.2119034982*r + 0.6806995451*g + 0.1073969566*b)
        let s = cbrt(0.0883024619*r + 0.2817188376*g + 0.6299787005*b)
        let a = 1.9779984951*l - 2.4285922050*m + 0.4505937099*s
        let q = 0.0259040371*l + 0.7827717662*m - 0.8086757660*s
        return TileLCH(l: 0.2104542553*l + 0.7936177850*m - 0.0040720468*s,
                       c: hypot(a,q), h: atan2(q,a) * 180 / .pi)
    }
    var lab: [Double] {
        let r = Self.linear(r), g = Self.linear(g), b = Self.linear(b)
        func f(_ v: Double) -> Double { v > 0.008856 ? cbrt(v) : 7.787*v + 16/116 }
        let x = f((0.4124*r + 0.3576*g + 0.1805*b) / 0.95047)
        let y = f(0.2126*r + 0.7152*g + 0.0722*b)
        let z = f((0.0193*r + 0.1192*g + 0.9505*b) / 1.08883)
        return [116*y-16, 500*(x-y), 200*(y-z)]
    }
    func deltaE(_ other: Self) -> Double {
        sqrt(zip(lab, other.lab).reduce(0) { $0 + pow($1.0 - $1.1, 2) })
    }
}

struct TileLCH: Equatable, Sendable {
    var l: Double
    var c: Double
    var h: Double
    func tuned(dark: Bool) -> Self {
        Self(l: min(dark ? 0.76 : 0.80, max(dark ? 0.56 : 0.68, l)), c: min(0.17, c * 0.8), h: h)
    }
    var rgb: TileRGB {
        var chroma = c
        var linear: [Double] = []
        repeat {
            let a = chroma * cos(h * .pi / 180), b = chroma * sin(h * .pi / 180)
            let x = pow(l + 0.3963377774*a + 0.2158037573*b, 3)
            let y = pow(l - 0.1055613458*a - 0.0638541728*b, 3)
            let z = pow(l - 0.0894841775*a - 1.2914855480*b, 3)
            linear = [4.0767416621*x - 3.3077115913*y + 0.2309699292*z,
                      -1.2684380046*x + 2.6097574011*y - 0.3413193965*z,
                      -0.0041960863*x - 0.7034186147*y + 1.7076147010*z]
            chroma = max(0, chroma - 0.004)
        } while linear.contains(where: { $0 < -0.0005 || $0 > 1.0005 }) && chroma > 0
        let encoded = linear.map { value in
            let v = min(1, max(0, value))
            return v <= 0.0031308 ? 12.92*v : 1.055*pow(v, 1/2.4)-0.055
        }
        return TileRGB(r: encoded[0], g: encoded[1], b: encoded[2])
    }
}

enum TileColors {
    static func hashed(_ key: String) -> TileLCH {
        let hash = key.utf8.reduce(UInt64(5381)) { ($0 &* 33) &+ UInt64($1) }
        return TileLCH(l: 0.67, c: 0.16, h: Double(hash % 360))
    }

    static func dominant(_ pixels: [TileRGB], alpha: [Double]) -> TileLCH? {
        var bins = Array(repeating: (weight: 0.0, r: 0.0, g: 0.0, b: 0.0), count: 36)
        for (pixel, alpha) in zip(pixels, alpha) where alpha >= 0.5 {
            let high = max(pixel.r, pixel.g, pixel.b), low = min(pixel.r, pixel.g, pixel.b)
            guard high > 0.12, low < 0.92, high - low > 0.08 else { continue }
            let saturation = (high - low) / high
            let hue: Double
            if high == pixel.r { hue = (pixel.g - pixel.b) / (high - low) }
            else if high == pixel.g { hue = (pixel.b - pixel.r) / (high - low) + 2 }
            else { hue = (pixel.r - pixel.g) / (high - low) + 4 }
            let bin = Int((hue * 60 + 360).truncatingRemainder(dividingBy: 360) / 10)
            bins[bin].weight += saturation
            bins[bin].r += pixel.r * saturation
            bins[bin].g += pixel.g * saturation
            bins[bin].b += pixel.b * saturation
        }
        guard let best = bins.max(by: { $0.weight < $1.weight }), best.weight > 0 else { return nil }
        return TileRGB(r: best.r/best.weight, g: best.g/best.weight, b: best.b/best.weight).lch
    }

    static func separate(_ seeds: [(String, TileLCH)], dark: Bool) -> [String: TileRGB] {
        var placed: [TileRGB] = []
        var result: [String: TileRGB] = [:]
        for (key, seed) in seeds {
            if seed.c == 0 { result[key] = seed.rgb; continue }
            var best = seed.rgb
            var bestDistance = -1.0
            let lower = dark ? 0.56 : 0.68, upper = dark ? 0.76 : 0.80
            let candidates = [seed.l, seed.l + 0.09, seed.l - 0.09, seed.l + 0.17, seed.l - 0.17, lower, upper]
            for l in candidates where l >= lower && l <= upper {
                let candidate = TileLCH(l: l, c: seed.c, h: seed.h).rgb
                let distance = placed.map { candidate.deltaE($0) }.min() ?? 99
                if distance > bestDistance { best = candidate; bestDistance = distance }
                if distance >= 20 { break }
            }
            result[key] = best
            placed.append(best)
        }
        return result
    }
}

@MainActor final class TileIconSampler {
    static let shared = TileIconSampler()
    private var seeds: [String: TileLCH] = [:]

    func seed(key: String, ids: [String]) -> TileLCH {
        if let seed = seeds[key] { return seed }
        guard let url = ids.lazy.compactMap({ NSWorkspace.shared.urlForApplication(withBundleIdentifier: $0) }).first else {
            let seed = TileColors.hashed(key)
            seeds[key] = seed
            return seed
        }
        let icon = NSWorkspace.shared.icon(forFile: url.path)
        let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 32, pixelsHigh: 32,
                                      bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
                                      isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 128, bitsPerPixel: 32)!
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
        NSGraphicsContext.current!.cgContext.clear(CGRect(x: 0, y: 0, width: 32, height: 32))
        icon.draw(in: NSRect(x: 0, y: 0, width: 32, height: 32))
        NSGraphicsContext.restoreGraphicsState()
        var pixels: [TileRGB] = []
        var alpha: [Double] = []
        for y in 0..<32 { for x in 0..<32 {
            let color = bitmap.colorAt(x: x, y: y)!.usingColorSpace(.deviceRGB)!
            pixels.append(TileRGB(r: color.redComponent, g: color.greenComponent, b: color.blueComponent))
            alpha.append(color.alphaComponent)
        } }
        let seed = TileColors.dominant(pixels, alpha: alpha) ?? TileLCH(l: 0.37, c: 0, h: 0)
        seeds[key] = seed
        return seed
    }
}
