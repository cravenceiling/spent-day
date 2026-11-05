import Foundation
import CoreGraphics

@MainActor func treemapChecks() throws {
    func rects(_ actual: [CGRect], _ expected: [CGRect], _ name: String) {
        let matches = actual.count == expected.count && zip(actual, expected).allSatisfy {
            abs($0.minX - $1.minX) < 0.00001 && abs($0.minY - $1.minY) < 0.00001 &&
            abs($0.width - $1.width) < 0.00001 && abs($0.height - $1.height) < 0.00001
        }
        check(matches, name + " got \(actual)")
    }
    rects(squarify(values: [6,6,4,3,2,2,1], in: CGRect(x: 0,y: 0,width: 6,height: 4)), [
        CGRect(x: 0,y: 0,width: 3,height: 2), CGRect(x: 0,y: 2,width: 3,height: 2),
        CGRect(x: 3,y: 0,width: 12.0/7,height: 7.0/3), CGRect(x: 33.0/7,y: 0,width: 9.0/7,height: 7.0/3),
        CGRect(x: 3,y: 7.0/3,width: 1.2,height: 5.0/3),
        CGRect(x: 4.2,y: 7.0/3,width: 1.2,height: 5.0/3),
        CGRect(x: 5.4,y: 7.0/3,width: 0.6,height: 5.0/3),
    ], "Bruls layout")
    rects(squarify(values: [3,1], in: CGRect(x: 0,y: 0,width: 100,height: 100)), [
        CGRect(x: 0,y: 0,width: 75,height: 100), CGRect(x: 75,y: 0,width: 25,height: 100)
    ], "three to one")
    rects(squarify(values: [0,1,0], in: CGRect(x: 5,y: 7,width: 30,height: 40)), [
        CGRect(x: 5,y: 7,width: 30,height: 40)
    ], "zeros dropped and single item fills")
    check(squarify(values: [0,0], in: .zero).isEmpty, "zero layout empty")
    rects([insetTile(CGRect(x: 0,y: 0,width: 100,height: 80), gap: 4)], [
        CGRect(x: 2,y: 2,width: 96,height: 76)
    ], "four point tile gap")
    rects(slice(values: [1,3], in: CGRect(x: 0,y: 0,width: 100,height: 50)), [
        CGRect(x: 0,y: 0,width: 25,height: 50), CGRect(x: 25,y: 0,width: 75,height: 50)
    ], "session slices retain chronology")
    rects(slice(values: [1,3], in: CGRect(x: 0,y: 0,width: 50,height: 100)), [
        CGRect(x: 0,y: 0,width: 50,height: 25), CGRect(x: 0,y: 25,width: 50,height: 75)
    ], "vertical session slices")
    let item = DayItem(key: "app:a", name: "A", kind: .app, appKey: "a", appIds: [], totalMs: 100,
                       sessions: [Session(startMs: 0, endMs: 100)])
    let device = DeviceDay(device: DeviceInfo(id: "a", name: "Device", platform: .macos, lastSeenMs: 0), totalMs: 100, items: [item])
    let detail = DayDetail(startMs: 0, endMs: 100, wallMs: 100, deviceMs: 100, devices: [device])
    let groups = deviceGroups(detail, in: CGRect(x: 0,y: 0,width: 100,height: 100))
    rects([groups[0].label, groups[0].tiles[0].1], [CGRect(x: 0,y: 0,width: 100,height: 24),
          CGRect(x: 0,y: 24,width: 100,height: 76)], "label band above item")
    let second = DeviceDay(device: DeviceInfo(id: "b", name: "Other device", platform: .linux, lastSeenMs: 0), totalMs: 100, items: [item])
    var pair = detail
    pair.devices.append(second)
    let paired = deviceGroups(pair, in: CGRect(x: 0,y: 0,width: 218,height: 100))
    rects(paired.map(\.label), [CGRect(x: 0,y: 0,width: 100,height: 24), CGRect(x: 118,y: 0,width: 100,height: 24)], "18 point group gap")
    var tiny = item
    tiny.key = "app:tiny"; tiny.totalMs = 2
    var deviceWithTiny = device
    deviceWithTiny.items.append(tiny)
    let collapsed = mapItems(deviceWithTiny, dayTotal: 1000)
    check(collapsed.count == 2 && collapsed.last?.item.name == "Other" && collapsed.last?.hidden.count == 1, "tiny items collapse into Other")
    check(Int64(89_999).accessibleDuration == "1 minute", "accessible duration rounds down below half a minute")
    check(Int64(90_000).accessibleDuration == "2 minutes", "accessible duration rounds up at half a minute")
    check(Int64(3_570_000).accessibleDuration == "1 hour 0 minutes", "accessible duration rounds across the hour")
    let demo = try DemoStore().day(query: dayQuery(Date(millis: 1_793_145_600_000)))
    check(demo.wallMs == 49_800_000 && demo.deviceMs == 51_300_000, "exact demo union 13h 50m and device sum 14h 15m")
    check(demo.devices.map(\.totalMs) == [32_460_000,14_040_000,4_800_000], "exact mockup device totals")
    check(demo.devices[0].items.first?.totalMs == 21_900_000, "demo Xcode 6h 5m")
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = TimeZone(identifier: "America/New_York")!
    let spring = calendar.date(from: DateComponents(year: 2026, month: 3, day: 8))!
    let fall = calendar.date(from: DateComponents(year: 2026, month: 11, day: 1))!
    let evening = calendar.date(from: DateComponents(year: 2026, month: 10, day: 28, hour: 23, minute: 38))!
    check(clock(evening.millis, calendar: calendar) == "23:38", "clock uses 24 hours regardless of locale")
    let springQuery = dayQuery(spring, calendar: calendar), fallQuery = dayQuery(fall, calendar: calendar)
    check(clock(springQuery.endMs, dayEnd: springQuery.endMs, calendar: calendar) == "24:00", "spring day end reads 24:00")
    check(clock(fallQuery.endMs, dayEnd: fallQuery.endMs, calendar: calendar) == "24:00", "fall day end reads 24:00")
    check(clock(springQuery.startMs, dayEnd: springQuery.endMs, calendar: calendar) == "00:00", "day start remains 00:00")
    check(springQuery.endMs - springQuery.startMs == 82_800_000, "DST spring day is 23 hours")
    check(fallQuery.endMs - fallQuery.startMs == 90_000_000, "DST fall day is 25 hours")
    let now = calendar.date(from: DateComponents(year: 2026, month: 11, day: 5, hour: 14, minute: 30))!
    let queries = usualQueries(day: now, now: now, calendar: calendar)
    check(queries.count == 4 && queries.allSatisfy { calendar.component(.hour, from: Date(millis: $0.endMs)) == 14 && calendar.component(.minute, from: Date(millis: $0.endMs)) == 30 }, "today's weekday samples stop at local current time")
    var empty = detail; empty.wallMs = 0
    var historical = detail; historical.wallMs = 300
    check(usualTotal([detail, historical, empty, empty]) == 200, "usual average ignores missing days")
    check(usualTotal([detail, empty, empty, empty]) == nil, "usual omitted with fewer than two samples")
}
