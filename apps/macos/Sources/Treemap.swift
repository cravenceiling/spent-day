import Foundation
import CoreGraphics

func squarify(values: [Double], in bounds: CGRect) -> [CGRect] {
    let positive = values.filter { $0 > 0 }
    let total = positive.reduce(0, +)
    guard total > 0, bounds.width > 0, bounds.height > 0 else { return [] }
    let areas = positive.map { $0 * bounds.width * bounds.height / total }
    var rect = bounds
    var output: [CGRect] = []
    var row: [Double] = []
    var index = 0
    func worst(_ row: [Double], _ side: Double) -> Double {
        guard let maximum = row.max(), let minimum = row.min(), side > 0 else { return .infinity }
        let sum = row.reduce(0, +)
        return max(side * side * maximum / (sum * sum), sum * sum / (side * side * minimum))
    }
    func place() {
        let sum = row.reduce(0, +)
        if rect.width >= rect.height {
            let width = sum / rect.height
            var y = rect.minY
            for area in row {
                let height = area / width
                output.append(CGRect(x: rect.minX, y: y, width: width, height: height))
                y += height
            }
            rect.origin.x += width
            rect.size.width = max(0, rect.width - width)
        } else {
            let height = sum / rect.width
            var x = rect.minX
            for area in row {
                let width = area / height
                output.append(CGRect(x: x, y: rect.minY, width: width, height: height))
                x += width
            }
            rect.origin.y += height
            rect.size.height = max(0, rect.height - height)
        }
    }
    while index < areas.count {
        let side = min(rect.width, rect.height)
        if row.isEmpty || worst(row + [areas[index]], side) <= worst(row, side) {
            row.append(areas[index])
            index += 1
        } else {
            place()
            row = []
        }
    }
    if !row.isEmpty { place() }
    return output
}

func slice(values: [Double], in rect: CGRect) -> [CGRect] {
    let values = values.filter { $0 > 0 }
    let total = values.reduce(0, +)
    guard total > 0 else { return [] }
    let horizontal = rect.width >= rect.height
    var position = horizontal ? rect.minX : rect.minY
    return values.map { value in
        let length = value / total * (horizontal ? rect.width : rect.height)
        defer { position += length }
        return horizontal
            ? CGRect(x: position, y: rect.minY, width: length, height: rect.height)
            : CGRect(x: rect.minX, y: position, width: rect.width, height: length)
    }
}

func insetTile(_ rect: CGRect, gap: CGFloat) -> CGRect {
    CGRect(x: rect.minX + gap / 2, y: rect.minY + gap / 2,
           width: max(0, rect.width - gap), height: max(0, rect.height - gap))
}

struct MapItem: Identifiable {
    let id: String
    let deviceID: String
    let item: DayItem
    var hidden: [DayItem] = []
    var session: Session?
    var dayEnd: Int64?
    var name: String { session.map { "\(clock($0.startMs, dayEnd: dayEnd)) to \(clock($0.endMs, dayEnd: dayEnd))" } ?? item.name }
    var totalMs: Int64 { session.map { $0.endMs - $0.startMs } ?? item.totalMs }
    var sessions: [Session] { session.map { [$0] } ?? item.sessions }
}

struct MapGroup: Identifiable {
    let device: DeviceDay
    let label: CGRect
    let tiles: [(MapItem, CGRect)]
    var id: String { device.device.id }
}

func mapItems(_ device: DeviceDay, dayTotal: Int64) -> [MapItem] {
    let small = device.items.filter { Double($0.totalMs) < Double(dayTotal) * 0.003 }
    var items = device.items.filter { Double($0.totalMs) >= Double(dayTotal) * 0.003 }
        .map { MapItem(id: device.device.id + "/" + $0.key, deviceID: device.device.id, item: $0) }
    if !small.isEmpty {
        let other = DayItem(key: "other", name: "Other", kind: .app, appKey: "other", appIds: [],
                            totalMs: small.reduce(0) { $0 + $1.totalMs },
                            sessions: small.flatMap(\.sessions).sorted { $0.startMs < $1.startMs })
        items.append(MapItem(id: device.device.id + "/other", deviceID: device.device.id, item: other, hidden: small))
    }
    return items.sorted { $0.totalMs == $1.totalMs ? $0.id < $1.id : $0.totalMs > $1.totalMs }
}

func deviceGroups(_ detail: DayDetail, in bounds: CGRect) -> [MapGroup] {
    let devices = detail.devices.sorted { $0.totalMs == $1.totalMs ? $0.device.id < $1.device.id : $0.totalMs > $1.totalMs }
    let boxes = squarify(values: devices.map { Double($0.totalMs) }, in: bounds.insetBy(dx: -9, dy: -9))
    return zip(devices, boxes).map { device, box in
        let rect = insetTile(box, gap: 18)
        let items = mapItems(device, dayTotal: detail.wallMs)
        let inner = CGRect(x: rect.minX - 2, y: rect.minY + 22, width: rect.width + 4, height: max(0, rect.height - 20))
        let tiles = zip(items, squarify(values: items.map { Double($0.totalMs) }, in: inner))
            .map { ($0.0, insetTile($0.1, gap: 4)) }
        return MapGroup(device: device, label: CGRect(x: rect.minX, y: rect.minY, width: rect.width, height: 24), tiles: tiles)
    }
}

func dayQuery(_ day: Date, calendar: Calendar = .current) -> DayQuery {
    let start = calendar.startOfDay(for: day)
    let end = calendar.date(byAdding: .day, value: 1, to: start)!
    return DayQuery(startMs: start.millis, endMs: end.millis)
}

func usualQueries(day: Date, now: Date, calendar: Calendar = .current) -> [DayQuery] {
    let today = calendar.isDate(day, inSameDayAs: now)
    let time = calendar.dateComponents([.hour, .minute, .second], from: now)
    return (1...4).map { week in
        let previous = calendar.date(byAdding: .day, value: -7 * week, to: day)!
        var query = dayQuery(previous, calendar: calendar)
        if today {
            query.endMs = calendar.date(bySettingHour: time.hour!, minute: time.minute!, second: time.second!, of: previous)!.millis
        }
        return query
    }
}

func usualTotal(_ days: [DayDetail]) -> Int64? {
    let totals = days.map(\.wallMs).filter { $0 > 0 }
    return totals.count >= 2 ? totals.reduce(0, +) / Int64(totals.count) : nil
}

func clock(_ millis: Int64, dayEnd: Int64? = nil, calendar: Calendar = .current) -> String {
    if millis == dayEnd { return "24:00" }
    let parts = calendar.dateComponents([.hour, .minute], from: Date(millis: millis))
    return String(format: "%02d:%02d", parts.hour!, parts.minute!)
}

func unionSessions(_ sessions: [Session]) -> [Session] {
    var result: [Session] = []
    for session in sessions.sorted(by: { $0.startMs < $1.startMs }) {
        if let last = result.last, session.startMs <= last.endMs {
            result[result.count - 1].endMs = max(last.endMs, session.endMs)
        } else { result.append(session) }
    }
    return result
}
