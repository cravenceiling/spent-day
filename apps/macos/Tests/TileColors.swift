import Foundation

@MainActor func tileColorChecks() {
    check(abs(TileRGB.black.contrast(.white) - 21) < 0.00001, "black white contrast 21")
    let red = TileRGB(hex: 0xff0000)
    let roundtrip = red.lch.rgb
    check(abs(roundtrip.r - 1) < 0.001 && roundtrip.g < 0.001 && roundtrip.b < 0.001, "OKLCH red round trip")
    let dark = TileLCH(l: 1, c: 1, h: 25).tuned(dark: true)
    let light = TileLCH(l: 0, c: 1, h: 25).tuned(dark: false)
    check(dark.l == 0.76 && dark.c == 0.17 && light.l == 0.68 && light.c == 0.17, "appearance bands and chroma clamp")
    check(TileColors.hashed("site:example.org").h == 110, "literal hashed hue")
    let sampled = TileColors.dominant([.white,.black,red,red,TileRGB(hex: 0x0000ff)], alpha: [1,1,1,1,0.1])
    check(sampled != nil && abs(sampled!.h - red.lch.h) < 0.001, "transparent and neutral pixels ignored")
    check(TileColors.dominant([.white,.black], alpha: [1,1]) == nil, "unsaturated icon becomes neutral")
    let neutral = TileLCH(l: 0.32, c: 0, h: 0)
    check(TileColors.separate([("neutral", neutral)], dark: true)["neutral"] == neutral.rgb, "neutral keeps its lightness")
    let same = TileLCH(l: 0.56, c: 0.12, h: 250)
    let separated = TileColors.separate([("large",same),("small",same)], dark: true)
    check(separated["large"] == same.rgb, "largest app keeps colour")
    check(separated["large"]!.deltaE(separated["small"]!) >= 20, "similar apps separate by deltaE 20")
    let crowded = TileColors.separate((0..<12).map { ("\($0)", TileLCH(l: 0.65, c: 0.12, h: 250)) }, dark: true)
    check(crowded.values.allSatisfy { $0.lch.l >= 0.559 && $0.lch.l <= 0.761 }, "crowded hues stay within the dark lightness band")
    let lightCrowded = TileColors.separate((0..<12).map { ("\($0)", TileLCH(l: 0.74, c: 0.12, h: 250)) }, dark: false)
    check(lightCrowded.values.allSatisfy { $0.lch.l >= 0.679 && $0.lch.l <= 0.801 }, "crowded hues stay within the light lightness band")
    check(TileLCH(l: 1, c: 0.1, h: 250).tuned(dark: false).l == 0.80, "light tuning caps lightness at 0.80")
    for dark in [true,false] {
        for hue in stride(from: 0.0, to: 360, by: 3) {
            for lightness in stride(from: 0.3, through: 0.95, by: 0.05) {
                let color = TileLCH(l: lightness, c: 0.17, h: hue).tuned(dark: dark).rgb
                if color.contrast(color.ink) < 4.5 || color.dimmed(dark: dark).contrast(color.ink) < 4.5 {
                    check(false, "label contrast hue \(hue), lightness \(lightness), dark \(dark)")
                    return
                }
            }
        }
    }
    check(true, "all labels pass 4.5 at rest and dimmed in light and dark")
}
