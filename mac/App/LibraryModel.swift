import AppKit
import Combine
import Foundation
import UniformTypeIdentifiers

struct LocalAlbum: Identifiable {
    let id: String
    let title: String
    let artist: String
    let count: Int
}

@MainActor
final class LibraryModel: ObservableObject {
    @Published var tracks: [LibraryDraft] = [] {
        didSet { albumArtistContexts = nil }
    }
    @Published var selection = Set<String>()
    @Published var albumFilter = ""
    @Published var busy = false
    @Published var message: String?
    @Published var error: String?
    @Published var location = ""
    @Published var remoteSession: LanSession?
    @Published var remoteNeedsReload = false
    @Published var artworkLoading = false
    @Published var showingLookup = false
    /// Find Album works on one album at a time; a selection spanning several
    /// albums becomes this queue of track sets, one per album.
    @Published private(set) var lookupGroups: [Set<String>] = []
    @Published private(set) var lookupIndex = 0
    @Published var lookupFolderName = ""
    @Published var showingMerge = false
    @Published var changesOnly = false {
        didSet { if changesOnly { reconcileVisibleSelection() } }
    }
    @Published var sortOrder: [KeyPathComparator<LibraryDraft>] = LibraryModel.defaultSortOrder
    private var remoteArtworkLoaded = Set<String>()
    private var remoteArtworkRequest: UUID?
    private var remoteArtworkOrder: [String] = []
    private var remoteSource = false
    private var albumArtistContexts: [String: [String: String]]?
    private let fallbackUndoManager: UndoManager = {
        let manager = UndoManager()
        manager.levelsOfUndo = 20
        return manager
    }()
    private weak var lastRemovalUndoManager: UndoManager?
    private var removalGeneration = 0

    private struct LibrarySnapshot {
        let tracks: [LibraryDraft]
        let selection: Set<String>
        let albumFilter: String
        let location: String
        let lookupFolderName: String
        let changesOnly: Bool
    }

    var dirtyCount: Int { tracks.filter(\.dirty).count }
    var hasChanges: Bool { dirtyCount > 0 }
    var updatesBlocked: Bool { busy || hasChanges }
    var isRemote: Bool { remoteSource }
    var canEdit: Bool { !busy && !remoteNeedsReload && (!isRemote || remoteSession != nil) }
    var canJoinAlbums: Bool { !isRemote || remoteSession?.supportsAlbumJoining == true }
    var canSave: Bool { hasChanges && canEdit }
    var canClearLibrary: Bool { !isRemote && !busy && !tracks.isEmpty }
    var canRemoveSelection: Bool { canClearLibrary && !selection.isEmpty }
    var canUndoRemoval: Bool {
        guard !isRemote, !busy, let manager = lastRemovalUndoManager, manager.canUndo else { return false }
        return ["Clear Library", "Remove Tracks"].contains(manager.undoActionName)
    }
    var selectedAlbumIDs: Set<String> { Set(selectedTracks.map(albumKey)) }
    var mergeTargets: [LocalAlbum] {
        let sources = selectedAlbumIDs
        return sources.count == 1 ? albums.filter { !sources.contains($0.id) } : albums
    }
    var canMergeSelection: Bool { canEdit && canJoinAlbums && editableCount > 0 && !mergeTargets.isEmpty }
    var canLookup: Bool {
        canEdit && visibleTaskTracks.contains { $0.original.writable }
    }
    static var defaultSortOrder: [KeyPathComparator<LibraryDraft>] {
        [KeyPathComparator(\LibraryDraft.sortArtist, comparator: String.StandardComparator.localizedStandard),
         KeyPathComparator(\LibraryDraft.sortAlbum, comparator: String.StandardComparator.localizedStandard),
         KeyPathComparator(\LibraryDraft.sortDisc), KeyPathComparator(\LibraryDraft.sortTrack),
         KeyPathComparator(\LibraryDraft.sortTitle, comparator: String.StandardComparator.localizedStandard)]
    }
    var albums: [LocalAlbum] {
        Dictionary(grouping: tracks, by: albumKey).map { key, rows in
            let first = rows[0]
            let title = first.sortAlbum
            let albumArtist = Self.displayText(first.effectiveAlbumArtist)
            let artists = Dictionary(grouping: rows.map { Self.displayText($0[.artist]) }.filter { !$0.isEmpty }, by: Self.normalizedAlbumText)
            let artist = !albumArtist.isEmpty ? albumArtist : (artists.count > 1 ? "Various Artists" : (artists.values.first?.first ?? ""))
            return LocalAlbum(id: key, title: title.isEmpty ? "Unknown album" : title,
                              artist: artist, count: rows.count)
        }.sorted {
            let title = $0.title.localizedStandardCompare($1.title)
            if title != .orderedSame { return title == .orderedAscending }
            let artist = $0.artist.localizedStandardCompare($1.artist)
            return artist == .orderedSame ? $0.id < $1.id : artist == .orderedAscending
        }
    }
    var visibleTracks: [LibraryDraft] {
        let rows = filteredTracks
        let descriptors = sortOrder.isEmpty ? Self.defaultSortOrder : sortOrder
        return rows.sorted { lhs, rhs in
            for descriptor in descriptors {
                let result: ComparisonResult
                if let key = descriptor.keyPath as? KeyPath<LibraryDraft, String> {
                    let comparison = lhs[keyPath: key].compare(rhs[keyPath: key], options: [.caseInsensitive, .diacriticInsensitive, .widthInsensitive, .numeric], locale: .current)
                    result = descriptor.order == .forward ? comparison : Self.reversed(comparison)
                } else {
                    result = descriptor.compare(lhs, rhs)
                }
                if result != .orderedSame { return result == .orderedAscending }
            }
            let path = lhs.id.localizedStandardCompare(rhs.id)
            return path == .orderedSame ? lhs.id < rhs.id : path == .orderedAscending
        }
    }
    private var filteredTracks: [LibraryDraft] {
        tracks.filter { (albumFilter.isEmpty || albumKey($0) == albumFilter) && (!changesOnly || $0.hasRecordedChanges) }
    }
    private var visibleTaskTracks: [LibraryDraft] {
        let visible = filteredTracks
        let selected = visible.filter { selection.contains($0.id) }
        return selected.isEmpty ? visible : selected
    }
    var selectedTracks: [LibraryDraft] { tracks.filter { selection.contains($0.id) } }
    var editableCount: Int { selectedTracks.filter { $0.original.writable }.count }
    private func albumKey(_ track: LibraryDraft) -> String {
        if isRemote && !track.albumIdentityIsModified,
           let group = track.original.albumGroup, !group.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return "group:\(group)"
        }
        if track.usesAlbumFolderFallback { return "folder:\(track.albumFolderPath)" }
        let album = Self.normalizedAlbumText(track.effectiveAlbum)
        let artist = Self.normalizedAlbumText(track.effectiveAlbumArtist)
        if !artist.isEmpty { return "tags:\(album)\u{0}artist:\(artist)" }
        let performers = albumPerformers(for: track)
        if performers.count == 1, let performer = performers.keys.first {
            return "tags:\(album)\u{0}artist:\(performer)"
        }
        // Compilations without an album artist remain folder-scoped until
        // Merge or Find Album supplies a common album identity.
        return "tags:\(album)\u{0}folder:\(track.albumFolderPath)"
    }
    private func artistContextKey(_ track: LibraryDraft) -> String {
        track.albumFolderPath + "\u{0}" + Self.normalizedAlbumText(track.effectiveAlbum)
    }
    private func albumPerformers(for track: LibraryDraft) -> [String: String] {
        if albumArtistContexts == nil {
            var contexts: [String: [String: String]] = [:]
            for row in tracks where !row.usesAlbumFolderFallback && Self.normalizedAlbumText(row.effectiveAlbumArtist).isEmpty {
                guard let performer = Self.meaningfulPerformer(row[.artist]) else { continue }
                contexts[artistContextKey(row), default: [:]][Self.normalizedAlbumText(performer)] = performer
            }
            albumArtistContexts = contexts
        }
        return albumArtistContexts?[artistContextKey(track)] ?? [:]
    }
    private static func meaningfulPerformer(_ value: String) -> String? {
        let name = displayText(value)
        return ["", "unknown", "unknown artist", "unknownartist", "untitled"].contains(normalizedAlbumText(name)) ? nil : name
    }
    private static func displayText(_ value: String) -> String {
        value.split(whereSeparator: \.isWhitespace).joined(separator: " ")
    }
    private static func normalizedAlbumText(_ value: String) -> String {
        displayText(value).folding(options: [.caseInsensitive, .diacriticInsensitive, .widthInsensitive], locale: Locale(identifier: "en_US_POSIX"))
    }
    private static func reversed(_ result: ComparisonResult) -> ComparisonResult {
        switch result {
        case .orderedAscending: return .orderedDescending
        case .orderedDescending: return .orderedAscending
        case .orderedSame: return .orderedSame
        }
    }
    private func followEditedAlbumFilter() {
        guard !albumFilter.isEmpty else { return }
        let selected = selectedAlbumIDs
        if selected.count == 1, let id = selected.first { albumFilter = id }
        else if !albums.contains(where: { $0.id == albumFilter }) { albumFilter = "" }
    }

    func choose() {
        guard !busy, !isRemote else { return }
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = true
        panel.prompt = "Open"
        panel.message = "Open an album folder or audio files to edit locally."
        if panel.runModal() == .OK { open(panel.urls) }
    }

    func confirmDiscard() -> Bool {
        guard !busy else { return false }
        guard hasChanges else { return true }
        let alert = NSAlert()
        alert.messageText = "Discard unsaved library changes?"
        alert.informativeText = "Your changes in \(location.isEmpty ? "this library" : location) have not been saved."
        alert.addButton(withTitle: "Keep Editing")
        alert.addButton(withTitle: "Discard Changes")
        return alert.runModal() == .alertSecondButtonReturn
    }

    func open(_ urls: [URL]) {
        guard !urls.isEmpty, !busy, !isRemote, confirmDiscard() else { return }
        busy = true
        error = nil
        message = nil
        Task {
            do {
                let result = try await Task.detached(priority: .userInitiated) { try LibraryEngine.read(urls) }.value
                if let failure = result.error { throw LibraryError(failure) }
                guard let records = result.tracks, !records.isEmpty else { throw LibraryError("No supported audio files were found.") }
                tracks = records.map(LibraryDraft.init)
                location = urls.count == 1 ? urls[0].lastPathComponent : "\(urls.count) locations"
                lookupFolderName = Self.folderName(for: records, sources: urls)
                invalidateRemovalUndo()
                showingLookup = false
                showingMerge = false
                changesOnly = false
                albumFilter = ""
                selection = Set(records.map(\.id))
                let skipped = result.skipped ?? []
                if !skipped.isEmpty { message = "Skipped \(skipped.count) files. " + skipped.prefix(3).joined(separator: ", ") }
            } catch { self.error = error.localizedDescription }
            busy = false
        }
    }

    func selectVisible() { selection = Set(filteredTracks.map(\.id)) }
    func filterChanged() { selectVisible() }
    private func selectTaskTargets() {
        selection.formIntersection(Set(filteredTracks.map(\.id)))
        if selection.isEmpty { selectVisible() }
    }
    private func reconcileVisibleSelection() {
        guard changesOnly else { return }
        selection.formIntersection(Set(filteredTracks.map(\.id)))
    }
    func beginLookup() {
        guard canEdit else { return }
        selectTaskTargets()
        let groups = albumGroups(selection)
        guard !groups.isEmpty else { return }
        lookupGroups = groups
        lookupIndex = 0
        showingMerge = false
        showingLookup = true
    }

    /// The album being looked up, or nil when the queue is finished.
    var currentLookupGroup: Set<String>? {
        showingLookup && lookupGroups.indices.contains(lookupIndex) ? lookupGroups[lookupIndex] : nil
    }

    /// Moves to the next album of the queue, or closes Find Album after the last.
    func nextLookup() {
        if lookupIndex + 1 < lookupGroups.count { lookupIndex += 1 }
        else { showingLookup = false }
    }

    /// Selected tracks grouped by album, in the table's order. Albums without
    /// an editable track are left out.
    func albumGroups(_ ids: Set<String>) -> [Set<String>] {
        var order: [String] = []
        var groups: [String: Set<String>] = [:]
        var editable = Set<String>()
        let visible = visibleTracks
        let visibleIDs = Set(visible.map(\.id))
        for track in visible + tracks.filter({ !visibleIDs.contains($0.id) }) where ids.contains(track.id) {
            let key = albumKey(track)
            if groups[key] == nil { order.append(key) }
            groups[key, default: []].insert(track.id)
            if track.original.writable { editable.insert(key) }
        }
        return order.filter(editable.contains).compactMap { groups[$0] }
    }
    func beginMerge() {
        guard canEdit else { return }
        selectTaskTargets()
        guard canMergeSelection else { return }
        error = nil
        showingLookup = false
        showingMerge = true
    }
    func mergeSelection(into albumID: String) {
        guard canMergeSelection else { return }
        let destination = tracks.filter { albumKey($0) == albumID }
        guard let target = destination.first(where: { !Self.displayText($0.effectiveAlbumArtist).isEmpty }) ?? destination.first else { return }
        guard !target.usesAlbumFolderFallback else {
            error = "Identify or name the destination album before merging tracks into it."
            message = nil
            return
        }
        let album = target.effectiveAlbum
        let harmonize = Self.displayText(target.effectiveAlbumArtist).isEmpty
        let joinsDestination = isRemote
        guard !(harmonize || joinsDestination) || destination.allSatisfy({ $0.original.writable }) else {
            error = "The destination includes read-only tracks. Choose a writable destination or identify its album artist first."
            message = nil
            return
        }
        let albumArtist: String
        if harmonize {
            let performers = Dictionary(grouping: destination.compactMap { Self.meaningfulPerformer($0[.artist]) }, by: Self.normalizedAlbumText)
            albumArtist = performers.count == 1 ? performers.values.first!.first! : "Various Artists"
        } else {
            albumArtist = target.effectiveAlbumArtist
        }
        let destinationIDs = Set(destination.map(\.id))
        var changed = 0
        for index in tracks.indices where tracks[index].original.writable {
            let selected = selection.contains(tracks[index].id)
            let stamp = (harmonize || joinsDestination) && destinationIDs.contains(tracks[index].id)
            guard selected || stamp else { continue }
            let beforeAlbum = tracks[index][.album], beforeArtist = tracks[index][.albumArtist]
            let beforeGrouping = tracks[index].albumGroupingChangeState
            if selected {
                tracks[index][.album] = album
                tracks[index][.albumArtist] = albumArtist
            } else if stamp {
                if joinsDestination { tracks[index][.album] = album }
                tracks[index][.albumArtist] = albumArtist
            }
            if isRemote { tracks[index].joinAlbum() }
            if tracks[index][.album] != beforeAlbum || tracks[index][.albumArtist] != beforeArtist || tracks[index].albumGroupingChangeState != beforeGrouping { changed += 1 }
        }
        showingMerge = false
        if !albumFilter.isEmpty, let destination = tracks.first(where: { $0.id == target.id }) { albumFilter = albumKey(destination) }
        reconcileVisibleSelection()
        error = nil
        message = changed == 0 ? "The selected tracks already belong to this album." : "Grouped \(changed) \(changed == 1 ? "track" : "tracks") into \(Self.displayText(album).isEmpty ? "the selected album" : Self.displayText(album)). Review the draft, then save changes."
    }

    /// Removes the loaded rows only. Their audio files are never deleted.
    func clearLibrary(undoManager: UndoManager? = nil) {
        guard canClearLibrary else { return }
        registerRemovalUndo(snapshot(), name: "Clear Library", manager: undoManager)
        tracks = []
        selection = []
        albumFilter = ""
        location = ""
        lookupFolderName = ""
        changesOnly = false
        showingLookup = false
        showingMerge = false
        error = nil
        message = "Library cleared. Your files remain on disk."
    }

    func removeSelection(undoManager: UndoManager? = nil) {
        guard canRemoveSelection else { return }
        let removed = tracks.filter { selection.contains($0.id) }.count
        guard removed > 0 else { return }
        registerRemovalUndo(snapshot(), name: "Remove Tracks", manager: undoManager)
        tracks.removeAll { selection.contains($0.id) }
        selection = []
        showingLookup = false
        showingMerge = false
        if tracks.isEmpty {
            albumFilter = ""
            location = ""
            lookupFolderName = ""
            changesOnly = false
        } else if !albums.contains(where: { $0.id == albumFilter }) {
            albumFilter = ""
        }
        error = nil
        message = "Removed \(removed) \(removed == 1 ? "track" : "tracks") from the library view. Your files remain on disk."
    }

    func undoRemoval() {
        guard canUndoRemoval else { return }
        lastRemovalUndoManager?.undo()
        objectWillChange.send()
    }

    private func snapshot() -> LibrarySnapshot {
        LibrarySnapshot(tracks: tracks, selection: selection, albumFilter: albumFilter, location: location, lookupFolderName: lookupFolderName, changesOnly: changesOnly)
    }

    private func registerRemovalUndo(_ snapshot: LibrarySnapshot, name: String, manager: UndoManager?) {
        let manager = manager ?? fallbackUndoManager
        let generation = removalGeneration
        lastRemovalUndoManager = manager
        let group = !manager.isUndoing && !manager.isRedoing
        if group { manager.beginUndoGrouping() }
        manager.registerUndo(withTarget: self) { [weak manager] model in
            guard model.removalGeneration == generation, !model.isRemote, !model.busy else { return }
            model.restoreRemoval(snapshot, name: name, manager: manager)
        }
        manager.setActionName(name)
        if group { manager.endUndoGrouping() }
    }

    private func restoreRemoval(_ previous: LibrarySnapshot, name: String, manager: UndoManager?) {
        registerRemovalUndo(snapshot(), name: name, manager: manager)
        // Rows that remained loaded can have newer edits or saved fingerprints.
        // Restoring removed rows must not rewind those surviving rows.
        let current = Dictionary(uniqueKeysWithValues: tracks.map { ($0.id, $0) })
        tracks = previous.tracks.map { current[$0.id] ?? $0 }
        selection = previous.selection.intersection(Set(tracks.map(\.id)))
        albumFilter = previous.albumFilter
        location = previous.location
        lookupFolderName = previous.lookupFolderName
        changesOnly = previous.changesOnly
        if !albumFilter.isEmpty && !albums.contains(where: { $0.id == albumFilter }) { albumFilter = "" }
        showingLookup = false
        showingMerge = false
        error = nil
        message = nil
    }

    private func invalidateRemovalUndo() {
        removalGeneration += 1
        fallbackUndoManager.removeAllActions(withTarget: self)
        lastRemovalUndoManager?.removeAllActions(withTarget: self)
        lastRemovalUndoManager = nil
    }

    private static func folderName(for records: [LibraryRecord], sources: [URL] = []) -> String {
        if sources.count == 1,
           (try? sources[0].resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true {
            return sources[0].lastPathComponent
        }
        let folders = Set(records.map { URL(fileURLWithPath: $0.path).deletingLastPathComponent().path })
        return folders.count == 1 ? URL(fileURLWithPath: folders.first!).lastPathComponent : ""
    }
    func commonValue(_ field: LibraryField) -> String {
        let rows = editableCount == 0 ? selectedTracks : selectedTracks.filter { $0.original.writable }
        guard let first = rows.first else { return "" }
        return rows.allSatisfy { $0[field] == first[field] } ? first[field] : ""
    }
    func isMixed(_ field: LibraryField) -> Bool {
        let rows = editableCount == 0 ? selectedTracks : selectedTracks.filter { $0.original.writable }
        return Set(rows.map { $0[field] }).count > 1
    }
    func set(_ field: LibraryField, to value: String) {
        guard canEdit else { return }
        for i in tracks.indices where selection.contains(tracks[i].id) && tracks[i].original.writable {
            if isRemote && (field == .album || field == .albumArtist) && tracks[i][field] != value {
                tracks[i].resetAlbumGrouping()
            }
            tracks[i][field] = value
        }
        if field == .album || field == .albumArtist || field == .artist { followEditedAlbumFilter() }
        reconcileVisibleSelection()
        message = nil
    }
    func setArtwork(_ artwork: LibraryArtwork) {
        guard canEdit else { return }
        for i in tracks.indices where selection.contains(tracks[i].id) && tracks[i].original.writable {
            tracks[i].artwork = artwork
            if !tracks[i].artworkIsModified { tracks[i].artwork = .keep }
        }
        reconcileVisibleSelection()
    }
    func chooseArtwork() {
        guard canEdit else { return }
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.jpeg, .png]
        panel.message = "Choose a cover for the selected tracks."
        guard panel.runModal() == .OK, let url = panel.url else { return }
        useArtwork(at: url)
    }
    /// A JPEG or PNG file, chosen or dropped on the cover, for the selected tracks.
    func useArtwork(at url: URL) {
        guard canEdit else { return }
        do {
            let type = try url.resourceValues(forKeys: [.contentTypeKey]).contentType
            guard type?.conforms(to: .jpeg) == true || type?.conforms(to: .png) == true else { throw LibraryError("Use a JPEG or PNG image for the cover.") }
            let size = try url.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0
            guard size <= 8 * 1024 * 1024 else { throw LibraryError("Choose a JPEG or PNG smaller than 8 MB.") }
            let data = try Data(contentsOf: url)
            guard NSImage(data: data) != nil else { throw LibraryError("This image could not be read.") }
            setArtwork(.replace(data))
        } catch { self.error = error.localizedDescription }
    }
    func revertSelection() {
        guard !busy else { return }
        for i in tracks.indices where selection.contains(tracks[i].id) { tracks[i].revert() }
        followEditedAlbumFilter()
        reconcileVisibleSelection()
        error = nil
        message = nil
        requestArtwork()
    }

    func save() {
        guard canSave else { return }
        do {
            let drafts = tracks.filter(\.dirty)
            busy = true
            error = nil
            message = nil
            let session = remoteSession
            if isRemote && session == nil { throw LibraryError("Reconnect to betterPod before saving.") }
            Task {
                do {
                    let outcome = try await Task.detached(priority: .userInitiated) {
                        try LibraryBatchWriter.save(drafts, session: session)
                    }.value
                    let result = outcome.result
                    if outcome.requiresReload { remoteNeedsReload = true }
                    if let failure = result.error { throw LibraryError(failure) }
                    let saved = Set(result.saved ?? [])
                    let joined = Set(result.joined ?? [])
                    // Only refresh successful files: failed drafts stay available for correction.
                    if !saved.isEmpty {
                        let urls = saved.map { URL(fileURLWithPath: $0) }
                        let refreshed = try await Task.detached(priority: .userInitiated) {
                            if let session { return try LanEngine.read(session, paths: Array(saved)) }
                            return try LibraryEngine.read(urls)
                        }.value
                        let byPath = Dictionary(uniqueKeysWithValues: (refreshed.tracks ?? []).map { ($0.path, $0) })
                        for i in tracks.indices where saved.contains(tracks[i].id) {
                            if let record = byPath[tracks[i].id] {
                                remoteArtworkLoaded.remove(tracks[i].id)
                                tracks[i].markSaved(record, artworkWasRead: session == nil, albumJoinWasSaved: joined.contains(tracks[i].id))
                            }
                        }
                        let missing = saved.subtracting(byPath.keys)
                        if !missing.isEmpty { throw LibraryError("Files were saved but could not be reloaded. Reopen the folder before editing them again.") }
                    }
                    message = "Saved \(saved.count) \(saved.count == 1 ? "track" : "tracks") \(isRemote ? "on betterPod" : "locally")."
                    if let warnings = result.warnings, !warnings.isEmpty { message! += " " + warnings.joined(separator: " ") }
                    let failures = result.failures ?? []
                    if !failures.isEmpty {
                        self.error = failures.map { URL(fileURLWithPath: $0.path).lastPathComponent + ": " + $0.error }.joined(separator: "\n")
                    }
                    if tracks.contains(where: { saved.contains($0.id) && $0.joinsAlbum }) {
                        self.error = [self.error, "Metadata was saved, but betterPod did not confirm album grouping. The grouping draft is still pending; review the warning and save again."].compactMap { $0 }.joined(separator: "\n")
                    }
                    if !albums.contains(where: { $0.id == albumFilter }) { albumFilter = "" }
                    reconcileVisibleSelection()
                } catch {
                    self.error = error.localizedDescription
                    if session != nil { remoteNeedsReload = true }
                }
                busy = false
                requestArtwork()
            }
        } catch { self.error = error.localizedDescription; busy = false }
    }

    func orderedTargets(_ ids: Set<String>) -> [LibraryDraft] {
        let rows = tracks.filter { ids.contains($0.id) }
        let filenamePositions = rows.compactMap { row -> (String, FilenamePosition)? in
            Self.filenamePosition(row.id).map { (row.id, $0) }
        }
        if !rows.isEmpty, filenamePositions.count == rows.count,
           Set(filenamePositions.map { $0.1 }).count == rows.count {
            let byDisc = Dictionary(grouping: filenamePositions.map { $0.1 }, by: \.disc)
            let complete = byDisc.values.allSatisfy { positions in
                Set(positions.map(\.track)) == Set(1...positions.count)
            }
            if complete {
                let positions = Dictionary(filenamePositions, uniquingKeysWith: { first, _ in first })
                return rows.sorted {
                    let a = positions[$0.id]!, b = positions[$1.id]!
                    return a.disc == b.disc ? a.track < b.track : a.disc < b.disc
                }
            }
        }
        let numbers = rows.map { "\($0.sortDisc)-\($0.sortTrack)" }
        let numbered = rows.allSatisfy { $0.sortTrack > 0 } && Set(numbers).count == rows.count
        return rows.sorted {
            if numbered {
                if $0.sortDisc != $1.sortDisc { return $0.sortDisc < $1.sortDisc }
                if $0.sortTrack != $1.sortTrack { return $0.sortTrack < $1.sortTrack }
            }
            return $0.id.localizedStandardCompare($1.id) == .orderedAscending
        }
    }

    private struct FilenamePosition: Hashable {
        let disc: Int
        let track: Int
    }
    private static let filenameDiscTrack = try! NSRegularExpression(pattern: #"^([0-9]{1,3})-([0-9]{1,3})(?:[\s._-]+|$)"#)
    private static let filenameTrack = try! NSRegularExpression(pattern: #"^([0-9]{1,3})(?:[\s._-]+|$)"#)
    private static let folderDisc = try! NSRegularExpression(pattern: #"^(?:cd|disc|disk)[\s._-]*([0-9]{1,3})$"#, options: [.caseInsensitive])

    private static func filenamePosition(_ path: String) -> FilenamePosition? {
        let url = URL(fileURLWithPath: path)
        let name = url.deletingPathExtension().lastPathComponent.trimmingCharacters(in: .whitespaces)
        func numbers(_ expression: NSRegularExpression, in text: String) -> [Int]? {
            let string = text as NSString
            guard let match = expression.firstMatch(in: text, range: NSRange(location: 0, length: string.length)) else { return nil }
            var result: [Int] = []
            for index in 1..<match.numberOfRanges {
                guard let number = Int(string.substring(with: match.range(at: index))), number > 0 else { return nil }
                result.append(number)
            }
            return result
        }
        if let position = numbers(filenameDiscTrack, in: name) {
            return FilenamePosition(disc: position[0], track: position[1])
        }
        guard let track = numbers(filenameTrack, in: name)?.first else { return nil }
        let parent = url.deletingLastPathComponent().lastPathComponent
        let disc = numbers(folderDisc, in: parent)?.first ?? 1
        return FilenamePosition(disc: disc, track: track)
    }

    func apply(_ details: AlbumLookupDetails, to ids: Set<String>, includeTracks: Bool, includeArtwork: Bool, includeMetadata: Bool = true) {
        guard canEdit else { return }
        let ordered = orderedTargets(ids)
        let source = details.tracks.sorted { $0.disc == $1.disc ? $0.track < $1.track : $0.disc < $1.disc }
        let mapping = includeTracks && source.count == ordered.count ? Dictionary(uniqueKeysWithValues: zip(ordered, source).map { ($0.0.id, $0.1) }) : [:]
        for i in tracks.indices where ids.contains(tracks[i].id) && tracks[i].original.writable {
            if includeMetadata {
                if !details.title.isEmpty { tracks[i][.album] = details.title }
                if !details.artist.isEmpty { tracks[i][.albumArtist] = details.artist }
                if let year = details.year, year > 0 { tracks[i][.year] = String(year) }
                if let genre = details.genre, !genre.isEmpty { tracks[i][.genre] = genre }
                if let match = mapping[tracks[i].id] {
                    if !match.title.isEmpty { tracks[i][.title] = match.title }
                    if !match.artist.isEmpty { tracks[i][.artist] = match.artist }
                    tracks[i][.track] = String(match.track)
                    tracks[i][.disc] = String(match.disc)
                    tracks[i][.trackTotal] = String(match.trackTotal)
                }
                if isRemote && (!details.title.isEmpty || !details.artist.isEmpty) {
                    if canJoinAlbums { tracks[i].joinAlbum() }
                    else { tracks[i].resetAlbumGrouping() }
                }
            }
            if includeArtwork, let cover = details.cover {
                tracks[i].artwork = .replace(cover)
                if !tracks[i].artworkIsModified { tracks[i].artwork = .keep }
            }
        }
        if includeMetadata { followEditedAlbumFilter() }
        reconcileVisibleSelection()
        message = "\(includeMetadata ? "Metadata" : "Cover") applied to the draft. Review it, then save changes."
        error = nil
    }

    func showRemote(_ session: LanSession, records: [LibraryRecord]) {
        remoteSource = true
        remoteSession = session
        remoteNeedsReload = false
        remoteArtworkLoaded = []
        remoteArtworkRequest = nil
        remoteArtworkOrder = []
        artworkLoading = false
        invalidateRemovalUndo()
        tracks = records.map { record in
            var draft = LibraryDraft(record)
            draft.artworkIsKnown = record.cover != nil
            return draft
        }
        location = session.name
        lookupFolderName = Self.folderName(for: records)
        showingLookup = false
        showingMerge = false
        changesOnly = false
        albumFilter = ""
        selection = []
        message = records.isEmpty ? "This betterPod library is empty. Add music and rescan on betterPod, then refresh." : nil
        error = nil
        busy = false
    }

    func remoteDisconnected() {
        remoteSession = nil
        remoteNeedsReload = true
        busy = false
        artworkLoading = false
        remoteArtworkRequest = nil
    }

    func discardRemoteDrafts() {
        for i in tracks.indices { tracks[i].revert() }
        reconcileVisibleSelection()
        error = nil
        message = nil
    }

    func requestArtwork() {
        guard !busy, !remoteNeedsReload, let session = remoteSession,
              let first = selectedTracks.first, first.artwork == .keep,
              !remoteArtworkLoaded.contains(first.id), remoteArtworkRequest == nil else { return }
        let path = first.id, modified = first.original.modified
        let token = UUID()
        remoteArtworkRequest = token
        artworkLoading = true
        Task {
            defer {
                if remoteArtworkRequest == token {
                    remoteArtworkRequest = nil
                    artworkLoading = false
                    if remoteSession?.id == session.id, selectedTracks.first?.id != path { requestArtwork() }
                }
            }
            do {
                let data = try await Task.detached(priority: .utility) { try LanEngine.cover(session, path: path) }.value
                guard remoteSession?.id == session.id, remoteArtworkRequest == token else { return }
                if let index = tracks.firstIndex(where: { $0.id == path && $0.original.modified == modified }) {
                    tracks[index].original.cover = data
                    tracks[index].artworkIsKnown = true
                    remoteArtworkLoaded.insert(path)
                    remoteArtworkOrder.removeAll { $0 == path }
                    remoteArtworkOrder.append(path)
                    trimArtworkCache(keeping: path)
                    reconcileVisibleSelection()
                }
            } catch {
                guard remoteSession?.id == session.id else { return }
                self.error = "The cover could not be loaded. " + error.localizedDescription
            }
        }
    }

    private func trimArtworkCache(keeping path: String) {
        func cacheBytes() -> Int { tracks.reduce(0) { $0 + ($1.original.cover?.count ?? 0) } }
        while remoteArtworkOrder.count > 4 || cacheBytes() > 16 * 1024 * 1024 {
            guard let index = remoteArtworkOrder.firstIndex(where: { $0 != path }) else { break }
            let oldest = remoteArtworkOrder.remove(at: index)
            remoteArtworkLoaded.remove(oldest)
            if let track = tracks.firstIndex(where: { $0.id == oldest }) {
                tracks[track].original.cover = nil
                tracks[track].artworkIsKnown = false
            }
        }
    }
}
