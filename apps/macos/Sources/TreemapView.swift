import AppKit
import SwiftUI

struct TreemapView: View {
    let detail: DayDetail
    let colors: [String: TileRGB]
    @Binding var drill: DayDrill
    @Binding var hovered: String?
    @State private var popoverID: String?
    @State private var hoverTask: Task<Void, Never>?
    @FocusState private var focused: String?
    @Environment(\.colorScheme) private var scheme
    @Environment(\.accessibilityReduceMotion) private var reducedMotion

    init(detail: DayDetail, colors: [String: TileRGB], drill: Binding<DayDrill>, hovered: Binding<String?>) {
        self.detail = detail; self.colors = colors
        _drill = drill; _hovered = hovered
        _popoverID = State(initialValue: hovered.wrappedValue)
    }

    private func cells(in rect: CGRect) -> [(MapItem, CGRect)] {
        guard let device = detail.devices.first(where: { $0.device.id == drill.deviceID }) else {
            return deviceGroups(detail, in: rect).flatMap(\.tiles)
        }
        let bounds = rect.insetBy(dx: -2, dy: -2)
        if let item = device.items.first(where: { $0.key == drill.itemKey }) {
            let sessions = item.sessions.sorted { $0.startMs < $1.startMs }
            let boxes = slice(values: sessions.map { Double($0.endMs - $0.startMs) }, in: bounds)
            return zip(sessions, boxes).enumerated().map { index, pair in
                (MapItem(id: device.device.id + "/" + item.key + "/\(index)", deviceID: device.device.id,
                         item: item, session: pair.0, dayEnd: detail.endMs), insetTile(pair.1, gap: 4))
            }
        }
        let items = mapItems(device, dayTotal: detail.wallMs)
        return zip(items, squarify(values: items.map { Double($0.totalMs) }, in: bounds))
            .map { ($0.0, insetTile($0.1, gap: 4)) }
    }

    var body: some View {
        GeometryReader { proxy in
            let bounds = CGRect(origin: .zero, size: proxy.size)
            let cells = cells(in: bounds)
            ZStack(alignment: .topLeading) {
                if drill.deviceID == nil {
                    ForEach(deviceGroups(detail, in: bounds)) { group in
                        Button {
                            navigate(DayDrill(deviceID: group.id))
                        } label: {
                            HStack(spacing: 6) {
                                Text(group.device.device.name).foregroundStyle(.primary)
                                Text(group.device.totalMs.durationText).foregroundStyle(.secondary)
                            }.font(.system(size: 13))
                        }.buttonStyle(.plain)
                            .accessibilityLabel("\(group.device.device.name), \(group.device.totalMs.accessibleDuration). Open device")
                            .frame(width: group.label.width, height: 20, alignment: .topLeading)
                            .offset(x: group.label.minX, y: group.label.minY)
                    }
                }
                ForEach(cells, id: \.0.id) { cell, rect in
                    tile(cell, rect: rect)
                        .transition(.opacity.combined(with: .scale(scale: reducedMotion ? 1 : 0.97)))
                }
                if let (cell, rect) = cells.first(where: { $0.0.id == popoverID }) {
                    let width = min(264.0, proxy.size.width)
                    let height = CGFloat(118 + min(5, cell.hidden.isEmpty ? cell.sessions.count : cell.hidden.count) * 22)
                    let x = min(max(0, rect.midX - width / 2), max(0, proxy.size.width - width))
                    let y = rect.maxY + 8 + height < proxy.size.height ? rect.maxY + 8
                        : rect.minY - 8 - height > 0 ? rect.minY - 8 - height
                        : min(max(0, proxy.size.height - height), rect.minY + min(90, rect.height / 3))
                    TilePopover(cell: cell, detail: detail, colors: colors)
                        .frame(width: width)
                        .offset(x: x, y: y)
                        .transition(.opacity.combined(with: .scale(scale: reducedMotion ? 1 : 0.97, anchor: .top)))
                        .zIndex(2)
                        .onHover { hover(cell.id, inside: $0, immediate: true) }
                }
            }
            .onMoveCommand { direction in
                let ids = cells.map { $0.0.id }
                guard !ids.isEmpty else { return }
                let index = focused.flatMap { ids.firstIndex(of: $0) } ?? -1
                let step = direction == .left || direction == .up ? -1 : 1
                focused = ids[(index + step + ids.count) % ids.count]
            }
        }
        .onAppear { popoverID = hovered }
        .onChange(of: drill) { _, _ in
            hoverTask?.cancel(); popoverID = nil; hovered = nil
        }
        .onDisappear { hoverTask?.cancel() }
    }

    private func tile(_ cell: MapItem, rect: CGRect) -> some View {
        let base = colors[cell.item.key] ?? TileColors.hashed(cell.item.key).tuned(dark: scheme == .dark).rgb
        let fill: TileRGB = {
            guard cell.session != nil, let index = Int(cell.id.split(separator: "/").last ?? "") else { return base }
            let lch = base.lch
            let offset = [0.0, -0.06, 0.04, -0.1, -0.02][index % 5]
            return TileLCH(l: min(0.9, max(0.3, lch.l + offset)), c: lch.c, h: lch.h).rgb
        }()
        let dim = hovered != nil && hovered != cell.id
        return Button { open(cell) } label: {
            RoundedRectangle(cornerRadius: min(10, rect.width / 3, rect.height / 3))
                .fill(Color(rgb: dim ? fill.dimmed(dark: scheme == .dark) : fill))
                .overlay(alignment: .topLeading) { TileLabel(name: cell.name, duration: cell.totalMs.durationText, size: rect.size, ink: fill.ink) }
                .overlay {
                    if focused == cell.id {
                        RoundedRectangle(cornerRadius: 10).stroke(Color(rgb: fill.ink), lineWidth: 2).padding(3)
                    }
                }
        }
        .buttonStyle(.plain).focusable().focused($focused, equals: cell.id)
        .accessibilityLabel("\(cell.item.name), \(cell.totalMs.accessibleDuration)\(cell.session.map { ", \(clock($0.startMs, dayEnd: detail.endMs)) to \(clock($0.endMs, dayEnd: detail.endMs))" } ?? "")")
        .accessibilityHint(cell.session != nil ? "Session" : cell.hidden.isEmpty ? "Opens sessions" : "Shows hidden items")
        .onKeyPress(.return) { open(cell); return .handled }
        .onHover { inside in hover(cell.id, inside: inside) }
        .frame(width: max(0, rect.width), height: max(0, rect.height))
        .offset(x: rect.minX, y: rect.minY)
        .animation(reducedMotion ? nil : .timingCurve(0.23, 1, 0.32, 1, duration: 0.15), value: dim)
    }

    private func open(_ cell: MapItem) {
        guard cell.session == nil else { return }
        if cell.hidden.isEmpty { navigate(DayDrill(deviceID: cell.deviceID, itemKey: cell.item.key)) }
        else { hover(cell.id, inside: true, immediate: true) }
    }

    private func navigate(_ path: DayDrill) {
        hoverTask?.cancel(); hovered = nil; popoverID = nil
        withAnimation(reducedMotion ? nil : .easeInOut(duration: 0.25)) { drill = path }
    }

    private func hover(_ id: String, inside: Bool, immediate: Bool = false) {
        hoverTask?.cancel()
        if inside {
            hovered = id
            let instant = immediate || popoverID != nil
            hoverTask = Task { @MainActor in
                if !instant { try? await Task.sleep(for: .milliseconds(300)) }
                guard !Task.isCancelled else { return }
                withAnimation(instant || reducedMotion ? nil : .timingCurve(0.23, 1, 0.32, 1, duration: 0.15)) { popoverID = id }
            }
        } else {
            hoverTask = Task { @MainActor in
                try? await Task.sleep(for: .milliseconds(90))
                guard !Task.isCancelled else { return }
                hovered = nil
                withAnimation(reducedMotion ? nil : .timingCurve(0.23, 1, 0.32, 1, duration: 0.15)) { popoverID = nil }
            }
        }
    }
}

private struct TileLabel: View {
    let name: String
    let duration: String
    let size: CGSize
    let ink: TileRGB

    private var tier: (horizontal: CGFloat, vertical: CGFloat, name: CGFloat, duration: CGFloat, both: Bool)? {
        func width(_ text: String, _ size: CGFloat, _ weight: NSFont.Weight) -> CGFloat {
            (text as NSString).size(withAttributes: [.font: NSFont.systemFont(ofSize: size, weight: weight)]).width
                * (text.contains(where: \.isNumber) ? 1.1 : 1)
        }
        let tiers: [(CGFloat, CGFloat, CGFloat)] = size.width >= 260 && size.height >= 200
            ? [(16,19,15),(13,15,13),(12,13,12.5)] : [(13,15,13),(12,13,12.5)]
        for (pad, ns, ds) in tiers {
            if size.width - 2 * pad >= max(width(name, ns, .semibold), width(duration, ds, .regular)) + 2,
               size.height - 2 * pad >= (ns + ds) * 1.4 + 2 { return (pad,pad,ns,ds,true) }
        }
        if size.width - 2 * 12 >= width(name,13,.semibold) + 2,
           size.height - 2 * 10 >= (name as NSString).size(withAttributes: [.font: NSFont.systemFont(ofSize: 13, weight: .semibold)]).height {
            return (12,10,13,0,false)
        }
        return nil
    }

    var body: some View {
        if let tier {
            VStack(alignment: .leading, spacing: 2) {
                Text(name).font(.system(size: tier.name, weight: .semibold)).tracking(-0.1)
                if tier.both { Text(duration).font(.system(size: tier.duration)).monospacedDigit() }
            }.fixedSize().foregroundStyle(Color(rgb: ink)).padding(.horizontal, tier.horizontal).padding(.vertical, tier.vertical)
        }
    }
}

private struct TilePopover: View {
    let cell: MapItem
    let detail: DayDetail
    let colors: [String: TileRGB]
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                RoundedRectangle(cornerRadius: 3).fill(Color(rgb: colors[cell.item.key] ?? TileColors.hashed(cell.item.key).rgb)).frame(width: 10, height: 10)
                Text(cell.item.name).font(.system(size: 14, weight: .semibold))
            }
            Text("\(cell.totalMs.durationText) on \(detail.devices.first { $0.device.id == cell.deviceID }?.device.name ?? "")")
                .font(.system(size: 12.5)).foregroundStyle(.secondary).padding(.top, 2)
            DayStrip(detail: detail, colors: colors, only: cell).frame(height: 28).padding(.top, 12).padding(.bottom, 8)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    if cell.hidden.isEmpty {
                        ForEach(Array(cell.sessions.enumerated()), id: \.offset) { _, session in
                            HStack { Text("\(clock(session.startMs, dayEnd: detail.endMs)) to \(clock(session.endMs, dayEnd: detail.endMs))"); Spacer(); Text((session.endMs - session.startMs).durationText).foregroundStyle(.secondary) }
                                .frame(height: 22)
                        }
                    } else {
                        ForEach(cell.hidden, id: \.key) { item in
                            HStack { Text(item.name); Spacer(); Text(item.totalMs.durationText).foregroundStyle(.secondary) }.frame(height: 22)
                        }
                    }
                }
            }.frame(height: CGFloat(min(5, cell.hidden.isEmpty ? cell.sessions.count : cell.hidden.count) * 22))
        }.font(.system(size: 12.5)).monospacedDigit().padding(14)
            .background(scheme == .dark ? Color(rgb: TileRGB(hex: 0x26262b)) : .white, in: RoundedRectangle(cornerRadius: 12))
            .shadow(color: .black.opacity(scheme == .dark ? 0.5 : 0.18), radius: 20, y: 14)
    }
}
