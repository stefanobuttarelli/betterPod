import Foundation

struct LanDevice: Decodable, Identifiable, Hashable, Sendable {
    let id: String
    let name: String
    let host: String
    let port: Int
}

struct LanSession: Sendable {
    let id: Int
    let name: String
    var supportsAlbumJoining = false
}

struct LanDiscovery: Decodable {
    let devices: [LanDevice]?
    let error: String?
}

private struct LanConnected: Decodable {
    let id: Int?
    let name: String?
    let error: String?
}

struct LanPage: Decodable {
    let tracks: [LibraryRecord]?
    let total: Int?
    let revision: String?
    let error: String?
    let nextOffset: Int?
    var capabilities: [String]? = nil
}

private struct LanCover: Decodable {
    let cover: Data?
    let error: String?
}

struct LanSaveOutcome {
    let result: LibrarySaveResult
    let requiresReload: Bool
}

enum LanEngine {
    private static func response(_ call: () -> UnsafeMutablePointer<CChar>?) throws -> Data {
        guard let pointer = call() else { throw LibraryError("The Wi-Fi connection did not respond.") }
        defer { BPFree(pointer) }
        return Data(String(cString: pointer).utf8)
    }

    static func discover() throws -> [LanDevice] {
        let result = try JSONDecoder().decode(LanDiscovery.self, from: response { BPLanDiscover() })
        if let error = result.error { throw LibraryError(error) }
        return result.devices ?? []
    }

    static func connect(device: LanDevice, code: String, name: String) throws -> LanSession {
        let data = try JSONSerialization.data(withJSONObject: ["host": device.host, "port": device.port,
                                                               "deviceID": device.id, "code": code, "name": name])
        let result = try String(decoding: data, as: UTF8.self).withCString { string in
            try JSONDecoder().decode(LanConnected.self, from: response {
                BPLanConnect(UnsafeMutablePointer(mutating: string))
            })
        }
        if let error = result.error { throw LibraryError(error) }
        guard let id = result.id, id > 0 else { throw LibraryError("The betterPod connection could not be opened.") }
        return LanSession(id: id, name: result.name ?? device.name)
    }

    static func disconnect(_ session: LanSession) { BPLanDisconnect(Int32(session.id)) }

    static func request<T: Decodable>(_ session: LanSession, object: [String: Any], as type: T.Type) throws -> T {
        try request(session, data: JSONSerialization.data(withJSONObject: object), as: type)
    }

    private static func request<T: Decodable>(_ session: LanSession, data: Data, as type: T.Type) throws -> T {
        try String(decoding: data, as: UTF8.self).withCString { string in
            try JSONDecoder().decode(type, from: response {
                BPLanRequest(Int32(session.id), UnsafeMutablePointer(mutating: string))
            })
        }
    }

    static func page(_ session: LanSession, offset: Int) throws -> LanPage {
        let result = try request(session, object: ["op": "list", "offset": offset, "limit": 256], as: LanPage.self)
        if let error = result.error { throw LibraryError(error) }
        return result
    }

    static func read(_ session: LanSession, paths: [String]) throws -> LibraryReadResult {
        var tracks: [LibraryRecord] = []
        var skipped: [String] = []
        for offset in stride(from: 0, to: paths.count, by: 256) {
            let batch = Array(paths[offset..<min(paths.count, offset + 256)])
            let result = try request(session, object: ["op": "read", "paths": batch], as: LibraryReadResult.self)
            if let error = result.error { throw LibraryError(error) }
            tracks += result.tracks ?? []
            skipped += result.skipped ?? []
        }
        return LibraryReadResult(tracks: tracks, skipped: skipped, error: nil)
    }

    static func cover(_ session: LanSession, path: String) throws -> Data? {
        let result = try request(session, object: ["op": "cover", "path": path], as: LanCover.self)
        if let error = result.error { throw LibraryError(error) }
        return result.cover
    }

    // Covers can occupy most of a frame. Split by encoded size and never retry
    // a request after a broken connection: its writes may already have landed.
    static func save(_ session: LanSession, request data: Data) throws -> LanSaveOutcome {
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let changes = object["tracks"] as? [[String: Any]] else {
            throw LibraryError("The selected changes could not be prepared.")
        }
        var batches: [[[String: Any]]] = []
        var batch: [[String: Any]] = []
        var size = 64
        for change in changes {
            let bytes = try JSONSerialization.data(withJSONObject: change).count + 1
            guard bytes < 15 * 1024 * 1024 else { throw LibraryError("A track's metadata is too large to send. Choose a smaller cover.") }
            if !batch.isEmpty && (size + bytes > 15 * 1024 * 1024 || batch.count == 50) {
                batches.append(batch)
                batch = []
                size = 64
            }
            batch.append(change)
            size += bytes
        }
        if !batch.isEmpty { batches.append(batch) }
        var saved: [String] = []
        var joined: [String] = []
        var failures: [LibrarySaveFailure] = []
        var warnings: [String] = []
        for (index, batch) in batches.enumerated() {
            do {
                let result = try request(session, object: ["op": "save", "tracks": batch], as: LibrarySaveResult.self)
                if let error = result.error { throw LibraryError(error) }
                saved += result.saved ?? []
                joined += result.joined ?? []
                failures += result.failures ?? []
                warnings += result.warnings ?? []
            } catch {
                let explanation = "The connection was interrupted. This batch may already have been saved. Reconnect and reload before trying again. " + error.localizedDescription
                for pending in batches[index...].flatMap({ $0 }) {
                    failures.append(LibrarySaveFailure(path: pending["path"] as? String ?? "", error: explanation))
                }
                return LanSaveOutcome(result: LibrarySaveResult(saved: saved, failures: failures, error: nil, warnings: warnings, joined: joined), requiresReload: true)
            }
        }
        return LanSaveOutcome(result: LibrarySaveResult(saved: saved, failures: failures, error: nil, warnings: warnings, joined: joined), requiresReload: false)
    }
}
