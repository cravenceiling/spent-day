import AppKit
import SwiftUI

/// The real icon when one of the app's ids is installed on this Mac, otherwise a
/// monogram (apps seen only on a phone or a Linux machine).
struct AppIcon: View {
    let ids: [String]
    let name: String
    var size: CGFloat = 22

    var body: some View {
        if let image = IconCache.shared.icon(ids: ids) {
            Image(nsImage: image)
                .resizable()
                .interpolation(.high)
                .frame(width: size, height: size)
        } else {
            RoundedRectangle(cornerRadius: size * 0.24, style: .continuous)
                .fill(Monogram.color(for: name).gradient)
                .frame(width: size * 0.84, height: size * 0.84)
                .overlay {
                    Text(name.prefix(1).uppercased())
                        .font(.system(size: size * 0.46, weight: .semibold, design: .rounded))
                        .foregroundStyle(.white)
                }
                .frame(width: size, height: size)
        }
    }
}

enum Monogram {
    private static let palette: [Color] = [.blue, .indigo, .purple, .pink, .orange, .teal, .green, .mint, .cyan, .brown]

    static func color(for name: String) -> Color {
        let hash = name.unicodeScalars.reduce(UInt32(5381)) { ($0 << 5) &+ $0 &+ $1.value }
        return palette[Int(hash % UInt32(palette.count))]
    }
}

@MainActor
final class IconCache {
    static let shared = IconCache()
    private var icons: [[String]: NSImage?] = [:]

    /// `ids` are the platform ids behind one summary row, most time first.
    func icon(ids: [String]) -> NSImage? {
        if let cached = icons[ids] { return cached }
        let workspace = NSWorkspace.shared
        let icon = ids.lazy
            .compactMap { workspace.urlForApplication(withBundleIdentifier: $0) }
            .first
            .map { workspace.icon(forFile: $0.path) }
        icons[ids] = icon
        return icon
    }
}

/// A thin proportional bar, its length relative to the largest row in the list.
struct UsageBar: View {
    let fraction: Double
    var tint: Color = .accentColor

    var body: some View {
        GeometryReader { proxy in
            Capsule()
                .fill(tint.opacity(0.85).gradient)
                .frame(width: max(3, proxy.size.width * fraction))
        }
        .frame(height: 4)
    }
}

/// A menu-style row for the menu bar window: highlights on hover like an NSMenu item.
struct MenuRowButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        Hover { hovering in
            configuration.label
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
                .background {
                    RoundedRectangle(cornerRadius: 5, style: .continuous)
                        .fill(hovering ? Color.primary.opacity(configuration.isPressed ? 0.16 : 0.09) : .clear)
                }
        }
    }

    private struct Hover<Label: View>: View {
        @Environment(\.isEnabled) private var isEnabled
        @State private var hovering = false
        @ViewBuilder var label: (Bool) -> Label

        var body: some View {
            label(hovering && isEnabled)
                .foregroundStyle(isEnabled ? .primary : .tertiary)
                .onHover { hovering = $0 }
        }
    }
}
