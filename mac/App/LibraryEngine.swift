import Foundation

enum LibraryField: String, CaseIterable, Identifiable {
    case title, artist, album, albumArtist, genre, year, track, trackTotal, disc
    var id: String { rawValue }
    var label: String {
        switch self {
        case .title: return "Title"
        case .artist: return "Artist"
        case .album: return "Album"
        case .albumArtist: return "Album artist"
        case .genre: return "Genre"
        case .year: return "Year"
        case .track: return "Track"
        case .trackTotal: return "Track total"
        case .disc: return "Disc"
        }
    }
    var numeric: Bool { [.year, .track, .trackTotal, .disc].contains(self) }
}

struct LibraryRecord: Decodable, Identifiable {
    let path: String
    let fileName: String
    let format: String
    let writable: Bool
    let title: String
    let artist: String
    let album: String
    let albumArtist: String
    let genre: String
    let year: Int
    let track: Int
    let trackTotal: Int
    let disc: Int
    var cover: Data?
    let size: Int64
    let modified: Int64
    let warning: String?
    var displayAlbum: String? = nil
    var displayAlbumArtist: String? = nil
    var albumGroup: String? = nil
    var displayAlbumID: String? = nil
    var id: String { path }
    var hasAlbumGroupingOverride: Bool {
        if let albumGroup, !albumGroup.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return true }
        return (displayAlbum != nil && displayAlbum != album) || (displayAlbumArtist != nil && displayAlbumArtist != albumArtist)
    }
    func value(_ field: LibraryField) -> String {
        switch field {
        case .title: return title
        case .artist: return artist
        case .album: return album
        case .albumArtist: return albumArtist
        case .genre: return genre
        case .year: return year == 0 ? "" : String(year)
        case .track: return track == 0 ? "" : String(track)
        case .trackTotal: return trackTotal == 0 ? "" : String(trackTotal)
        case .disc: return disc == 0 ? "" : String(disc)
        }
    }
}

struct LibraryReadResult: Decodable {
    let tracks: [LibraryRecord]?
    let skipped: [String]?
    let error: String?
}

struct LibrarySaveFailure: Decodable {
    let path: String
    let error: String
}

struct LibrarySaveResult: Decodable {
    let saved: [String]?
    let failures: [LibrarySaveFailure]?
    let error: String?
    var warnings: [String]? = nil
    var joined: [String]? = nil
}

enum LibraryArtwork: Equatable {
    case keep, replace(Data), remove
}

enum LibraryChangeState: String {
    case unchanged, modified, saved
}

struct LibraryDraft: Identifiable {
    var original: LibraryRecord
    var values: [LibraryField: String]
    var artwork: LibraryArtwork = .keep
    var artworkIsKnown = true
    private(set) var savedFieldPreviousValues: [LibraryField: String] = [:]
    private(set) var savedArtworkChanged = false
    private(set) var resetsAlbumGrouping = false
    private(set) var joinsAlbum = false
    private(set) var savedAlbumGroupingChanged = false
    var id: String { original.path }
    var albumIdentityIsModified: Bool { resetsAlbumGrouping || joinsAlbum || self[.album] != original.album || self[.albumArtist] != original.albumArtist }
    var effectiveAlbum: String { albumIdentityIsModified ? self[.album] : (original.displayAlbum ?? self[.album]) }
    var effectiveAlbumArtist: String { albumIdentityIsModified ? self[.albumArtist] : (original.displayAlbumArtist ?? self[.albumArtist]) }
    var sortTitle: String { Self.sortText(self[.title]).isEmpty ? original.fileName : Self.sortText(self[.title]) }
    var sortArtist: String {
        let albumArtist = Self.sortText(effectiveAlbumArtist)
        return albumArtist.isEmpty ? Self.sortText(self[.artist]) : albumArtist
    }
    var sortAlbum: String {
        usesAlbumFolderFallback ? albumFolderName : Self.sortText(effectiveAlbum)
    }
    var usesAlbumFolderFallback: Bool {
        let album = Self.sortText(effectiveAlbum).folding(options: [.caseInsensitive, .diacriticInsensitive, .widthInsensitive], locale: Locale(identifier: "en_US_POSIX"))
        return ["", "unknown", "unknown album", "unknownalbum", "untitled", "untitled album"].contains(album)
    }
    var albumFolderPath: String {
        var folder = URL(fileURLWithPath: id).deletingLastPathComponent().standardizedFileURL
        let name = folder.lastPathComponent
        let range = NSRange(name.startIndex..<name.endIndex, in: name)
        if Self.albumDiscFolder.firstMatch(in: name, range: range) != nil { folder.deleteLastPathComponent() }
        return folder.path
    }
    var albumFolderName: String { Self.sortText(URL(fileURLWithPath: albumFolderPath).lastPathComponent) }
    var sortDisc: Int { Int(self[.disc].trimmingCharacters(in: .whitespaces)) ?? 0 }
    var sortTrack: Int { Int(self[.track].trimmingCharacters(in: .whitespaces)) ?? 0 }
    var sortArtwork: Int {
        switch artwork {
        case .keep: return original.cover == nil ? 0 : 1
        case .replace: return 1
        case .remove: return 0
        }
    }
    var sortChanges: Int { dirty ? 2 : (hasRecordedChanges ? 1 : 0) }
    private static func sortText(_ value: String) -> String {
        value.split(whereSeparator: \.isWhitespace).joined(separator: " ")
    }
    private static let albumDiscFolder = try! NSRegularExpression(pattern: #"^(?:cd|disc|disk)[\s._-]*[0-9]{1,3}$"#, options: [.caseInsensitive])
    init(_ record: LibraryRecord) {
        original = record
        values = Dictionary(uniqueKeysWithValues: LibraryField.allCases.map { ($0, record.value($0)) })
    }
    subscript(_ field: LibraryField) -> String {
        get { values[field] ?? "" }
        set { values[field] = newValue }
    }
    var changedFields: Set<LibraryField> {
        Set(LibraryField.allCases.filter { self[$0] != original.value($0) })
    }
    var savedChangedFields: Set<LibraryField> { Set(savedFieldPreviousValues.keys) }
    var artworkIsModified: Bool {
        switch artwork {
        case .keep: return false
        case .replace(let data): return !artworkIsKnown || data != original.cover
        case .remove: return !artworkIsKnown || original.cover != nil
        }
    }
    var dirty: Bool { artworkIsModified || !changedFields.isEmpty || resetsAlbumGrouping || joinsAlbum }
    var hasRecordedChanges: Bool { dirty || !savedFieldPreviousValues.isEmpty || savedArtworkChanged || savedAlbumGroupingChanged }
    var albumGroupingChangeState: LibraryChangeState { resetsAlbumGrouping || joinsAlbum ? .modified : (savedAlbumGroupingChanged ? .saved : .unchanged) }
    mutating func resetAlbumGrouping() {
        if original.hasAlbumGroupingOverride { resetsAlbumGrouping = true }
    }
    mutating func joinAlbum() { joinsAlbum = true }
    func changeState(for field: LibraryField) -> LibraryChangeState {
        if self[field] != original.value(field) { return .modified }
        return savedFieldPreviousValues[field] == nil ? .unchanged : .saved
    }
    func previousValue(for field: LibraryField) -> String {
        changeState(for: field) == .modified ? original.value(field) : (savedFieldPreviousValues[field] ?? original.value(field))
    }
    var artworkChangeState: LibraryChangeState {
        artworkIsModified ? .modified : (savedArtworkChanged ? .saved : .unchanged)
    }
    mutating func revert() {
        values = Dictionary(uniqueKeysWithValues: LibraryField.allCases.map { ($0, original.value($0)) })
        artwork = .keep
        resetsAlbumGrouping = false
        joinsAlbum = false
    }
    mutating func markSaved(_ record: LibraryRecord, artworkWasRead: Bool = true, albumJoinWasSaved: Bool = false) {
        let pendingFields = changedFields
        let pendingArtwork = artworkIsModified
        let pendingGrouping = resetsAlbumGrouping
        let pendingJoin = joinsAlbum
        let priorArtwork = artwork
        let priorCover = original.cover
        let priorArtworkKnown = artworkIsKnown
        for field in pendingFields where record.value(field) != original.value(field) {
            savedFieldPreviousValues[field] = original.value(field)
        }
        if pendingArtwork { savedArtworkChanged = true }
        original = record
        if artworkWasRead {
            artworkIsKnown = true
        } else {
            switch priorArtwork {
            case .replace(let data) where pendingArtwork:
                original.cover = data
                artworkIsKnown = true
            case .remove where pendingArtwork:
                original.cover = nil
                artworkIsKnown = true
            default:
                original.cover = priorCover
                artworkIsKnown = priorArtworkKnown
            }
        }
        revert()
        if pendingJoin {
            if albumJoinWasSaved {
                savedAlbumGroupingChanged = true
            } else {
                joinsAlbum = true
            }
        }
        if pendingGrouping && !(pendingJoin && albumJoinWasSaved) {
            if record.hasAlbumGroupingOverride {
                resetsAlbumGrouping = true
            } else {
                savedAlbumGroupingChanged = true
            }
        }
    }
    var cover: Data? {
        switch artworkIsModified ? artwork : .keep {
        case .keep: return original.cover
        case .replace(let data): return data
        case .remove: return nil
        }
    }
    func fieldsObject() throws -> [String: Any] {
        var fields: [String: Any] = [:]
        for field in LibraryField.allCases where self[field] != original.value(field) || ((resetsAlbumGrouping || joinsAlbum) && (field == .album || field == .albumArtist)) {
            if field.numeric {
                let text = self[field].trimmingCharacters(in: .whitespaces)
                let value = text.isEmpty ? 0 : Int(text)
                guard let value, value >= 0, value <= (field == .year ? 9999 : 65535) else {
                    throw LibraryError("\(original.fileName): \(field.label) must be a whole number between 0 and \(field == .year ? 9999 : 65535).")
                }
                fields[field.rawValue] = value
            } else {
                guard self[field].utf8.count <= 65_536, !self[field].contains("\u{0}") else {
                    throw LibraryError("\(original.fileName): \(field.label) is too long or contains a null character.")
                }
                fields[field.rawValue] = self[field]
            }
        }
        return fields
    }
    func saveObject() throws -> [String: Any] {
        var object: [String: Any] = ["path": id, "fields": try fieldsObject(), "size": original.size, "modified": original.modified]
        if joinsAlbum { object["joinAlbum"] = true }
        switch artworkIsModified ? artwork : .keep {
        case .keep: break
        case .replace(let data): object["cover"] = data.base64EncodedString()
        case .remove: object["removeCover"] = true
        }
        return object
    }
}

struct LibraryError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

enum LibraryEngine {
    private static func invoke<T: Decodable>(_ json: Data, call: (UnsafeMutablePointer<CChar>) -> UnsafeMutablePointer<CChar>?) throws -> T {
        let string = String(decoding: json, as: UTF8.self)
        return try string.withCString { chars in
            guard let result = call(UnsafeMutablePointer(mutating: chars)) else {
                throw LibraryError("The library engine did not respond.")
            }
            defer { BPFree(result) }
            return try JSONDecoder().decode(T.self, from: Data(String(cString: result).utf8))
        }
    }
    static func read(_ urls: [URL]) throws -> LibraryReadResult {
        try invoke(JSONEncoder().encode(urls.map(\.path)), call: BPLibraryRead)
    }
    static func save(_ request: Data) throws -> LibrarySaveResult {
        try invoke(request, call: BPLibrarySave)
    }
}

enum LibraryBatchWriter {
    // Capture lightweight drafts on the UI, then validate and encode at most
    // one bounded batch on a worker. Shared artwork Data stays shared until its
    // batch is encoded, even when thousands of tracks use the same cover.
    static func save(_ drafts: [LibraryDraft], session: LanSession?) throws -> LanSaveOutcome {
        for draft in drafts {
            _ = try draft.fieldsObject()
            if case .replace(let cover) = draft.artwork, cover.isEmpty || cover.count > 8 * 1024 * 1024 {
                throw LibraryError("\(draft.original.fileName): choose a cover smaller than 8 MB.")
            }
        }
        var saved: [String] = []
        var failures: [LibrarySaveFailure] = []
        var warnings: [String] = []
        var joined: [String] = []
        var batch: [[String: Any]] = []
        var bytes = 64
        var submitted = 0
        func flush() throws -> Bool {
            let outcome: LanSaveOutcome = try autoreleasepool {
                let request = try JSONSerialization.data(withJSONObject: ["tracks": batch])
                if let session { return try LanEngine.save(session, request: request) }
                return LanSaveOutcome(result: try LibraryEngine.save(request), requiresReload: false)
            }
            if let error = outcome.result.error { throw LibraryError(error) }
            saved += outcome.result.saved ?? []
            failures += outcome.result.failures ?? []
            warnings += outcome.result.warnings ?? []
            joined += outcome.result.joined ?? []
            submitted += batch.count
            batch = []
            bytes = 64
            if outcome.requiresReload {
                failures += drafts.dropFirst(submitted).map {
                    LibrarySaveFailure(path: $0.id, error: "Not sent after the connection was interrupted. Reconnect and reload before saving.")
                }
            }
            return outcome.requiresReload
        }
        for draft in drafts {
            let object = try draft.saveObject()
            let count = try autoreleasepool { try JSONSerialization.data(withJSONObject: object).count + 1 }
            guard count < 15 * 1024 * 1024 else { throw LibraryError("\(draft.original.fileName): metadata is too large. Choose a smaller cover.") }
            if !batch.isEmpty && (bytes + count > 15 * 1024 * 1024 || batch.count == 50) {
                if try flush() { return LanSaveOutcome(result: LibrarySaveResult(saved: saved, failures: failures, error: nil, warnings: warnings, joined: joined), requiresReload: true) }
            }
            batch.append(object)
            bytes += count
        }
        let reload = batch.isEmpty ? false : try flush()
        return LanSaveOutcome(result: LibrarySaveResult(saved: saved, failures: failures, error: nil, warnings: warnings, joined: joined), requiresReload: reload)
    }
}
