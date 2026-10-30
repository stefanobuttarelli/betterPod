// Engine.swift — the Go engine (mac/core, linked as libbpshare.a): scans
// what was dropped and sends it with an 8-digit code. Everything crosses as
// JSON in C strings.

import AppKit
import Foundation

struct ScanAlbum: Decodable, Identifiable {
    let album: String
    let artist: String
    let songs: Int
    let cover: Data?
    var id: String { album + "\u{0}" + artist }
}

struct ScanResult: Decodable {
    let id: Int?
    let kind: String?
    let title: String?
    let artist: String?
    let songs: Int?
    let bytes: Int64?
    let cover: Data?
    let albums: [ScanAlbum]?
    let skipped: [String]?
    let m4a: Bool?
    let error: String?
}

struct ShareStatus: Decodable {
    let phase: String
    let code: String
    let done: Int64
    let total: Int64
    let rate: Double
    let error: String?
    let unplayable: [String]?
    let on: String?
}

enum Engine {
    private static func call(_ f: () -> UnsafeMutablePointer<CChar>?) -> Data? {
        guard let p = f() else { return nil }
        defer { BPFree(p) }
        return String(cString: p).data(using: .utf8)
    }

    /// Reads the files and folders (slow with many songs: not on the main thread).
    static func scan(_ urls: [URL]) -> ScanResult {
        let paths = urls.map { $0.path }
        let json = String(data: (try? JSONEncoder().encode(paths)) ?? Data("[]".utf8), encoding: .utf8) ?? "[]"
        let data = json.withCString { s in call { BPScan(UnsafeMutablePointer(mutating: s)) } }
        if let data, let r = try? JSONDecoder().decode(ScanResult.self, from: data) {
            return r
        }
        return ScanResult(id: nil, kind: nil, title: nil, artist: nil, songs: nil, bytes: nil, cover: nil,
                          albums: nil, skipped: nil, m4a: nil, error: "Couldn't read these files")
    }

    static func send(selection: Int, from: String) -> Int {
        Int(from.withCString { BPSend(Int32(selection), UnsafeMutablePointer(mutating: $0)) })
    }

    static func status(_ share: Int) -> ShareStatus? {
        guard let data = call({ BPStatus(Int32(share)) }) else { return nil }
        return try? JSONDecoder().decode(ShareStatus.self, from: data)
    }

    static func cancel(_ share: Int) {
        BPCancel(Int32(share))
    }
}

/// "30 MB", as the betterPod writes sizes.
func humanBytes(_ n: Int64) -> String {
    let f = ByteCountFormatter()
    f.countStyle = .file
    return f.string(fromByteCount: n)
}
