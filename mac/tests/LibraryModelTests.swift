import AppKit
import Foundation

@main
struct LibraryModelTests {
    @MainActor static func main() async throws {
        func record(_ path: String, title: String = "Original", artist: String = "Artist", track: Int = 0, writable: Bool = true, cover: Data? = nil,
                    album: String = "Album", albumArtist: String = "", disc: Int = 0, genre: String = "") -> LibraryRecord {
            LibraryRecord(path: path, fileName: URL(fileURLWithPath: path).lastPathComponent, format: writable ? "MP3" : "WAV",
                          writable: writable, title: title, artist: artist, album: album, albumArtist: albumArtist, genre: genre,
                          year: 2020, track: track, trackTotal: 0, disc: disc, cover: cover, size: 100, modified: 1000, warning: nil)
        }
        func check(_ condition: @autoclosure () -> Bool, _ message: String) {
            precondition(condition(), message)
        }
        let a = record("/album/02 second.mp3", track: 2)
        let b = record("/album/01 first.mp3", title: "First", artist: "Other", track: 1)
        let readonly = record("/album/read.wav", title: "Read only", writable: false)
        let coverData = Data(base64Encoded: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")!
        var provenance = LibraryDraft(a)
        check(provenance.changedFields.isEmpty && !provenance.hasRecordedChanges, "Newly loaded tracks must have no change marks")
        provenance[.title] = "Changed title"
        check(provenance.changeState(for: .title) == .modified && provenance.previousValue(for: .title) == "Original", "Pending fields must retain their previous value")
        check(provenance.changeState(for: .artist) == .unchanged, "Unedited fields must not be highlighted")
        provenance.artwork = .replace(coverData)
        check(provenance.artworkChangeState == .modified, "New cover artwork must be marked independently")
        provenance.markSaved(record(a.id, title: "Changed title", track: a.track, cover: coverData))
        check(!provenance.dirty && provenance.changeState(for: .title) == .saved && provenance.savedChangedFields == [.title], "Saving must clear dirty status while retaining exact field marks")
        check(provenance.previousValue(for: .title) == "Original" && provenance.artworkChangeState == .saved, "Saved metadata and cover changes must remain explainable")
        provenance[.title] = "Another title"
        check(provenance.previousValue(for: .title) == "Changed title", "New drafts compare against the current saved file")
        provenance.revert()
        check(provenance[.title] == "Changed title" && provenance.changeState(for: .title) == .saved && provenance.artworkChangeState == .saved,
              "Revert must discard only pending edits and preserve saved history")
        var sameCover = LibraryDraft(record(a.id, cover: coverData))
        sameCover.artwork = .replace(coverData)
        check(!sameCover.dirty && sameCover.artworkChangeState == .unchanged, "Applying the identical cover is not a replacement")
        let noCoverWrite = try sameCover.saveObject()
        check(noCoverWrite["cover"] == nil, "No-op artwork must not be sent to the writer")
        var emptyCover = LibraryDraft(a)
        emptyCover.artwork = .remove
        check(!emptyCover.dirty, "Removing a known-empty local cover must not create a false change")
        emptyCover.artworkIsKnown = false
        check(emptyCover.dirty && emptyCover.artworkChangeState == .modified, "An unknown remote cover must still support explicit removal")
        emptyCover.markSaved(a, artworkWasRead: false)
        check(emptyCover.artworkIsKnown && !emptyCover.dirty && emptyCover.artworkChangeState == .saved,
              "A confirmed remote cover removal must remain marked after its metadata refresh")
        var groupedRecord = record("/remote/album/01.mp3", album: "Known Album", albumArtist: "Known Artist")
        groupedRecord.albumGroup = "private-group"
        groupedRecord.displayAlbum = groupedRecord.album
        groupedRecord.displayAlbumArtist = groupedRecord.albumArtist
        var joinedDraft = LibraryDraft(groupedRecord)
        joinedDraft.artworkIsKnown = false
        joinedDraft.joinAlbum()
        let joinObject = try joinedDraft.saveObject()
        let joinFields = joinObject["fields"] as! [String: Any]
        check(joinedDraft.dirty && joinedDraft.changedFields.isEmpty && joinedDraft.albumGroupingChangeState == .modified &&
              joinObject["joinAlbum"] as? Bool == true && joinFields.count == 2 && joinFields["album"] as? String == "Known Album" && joinFields["albumArtist"] as? String == "Known Artist",
              "Joining an unchanged remote identity must remain visible and send both album fields with the join request")
        check(joinObject["cover"] == nil && joinObject["removeCover"] == nil, "A grouping-only save must preserve unknown remote artwork")
        joinedDraft.revert()
        check(!joinedDraft.dirty && joinedDraft.albumGroupingChangeState == .unchanged && !joinedDraft.albumIdentityIsModified,
              "Revert must restore the private grouping without staging file changes")
        joinedDraft.joinAlbum()
        joinedDraft.markSaved(groupedRecord, artworkWasRead: false)
        check(joinedDraft.dirty && joinedDraft.joinsAlbum && joinedDraft.albumGroupingChangeState == .modified,
              "Saving tags without a grouping acknowledgment must leave the join pending")
        groupedRecord.albumGroup = "remote-album:acknowledged"
        joinedDraft.markSaved(groupedRecord, artworkWasRead: false, albumJoinWasSaved: true)
        check(!joinedDraft.dirty && joinedDraft.albumGroupingChangeState == .saved && joinedDraft.savedChangedFields.isEmpty && !joinedDraft.artworkIsKnown,
              "Only an acknowledged group assignment may clear the pending join and record saved grouping history")
        joinedDraft[.title] = "Later metadata"
        joinedDraft.revert()
        check(joinedDraft.albumGroupingChangeState == .saved, "Later metadata edits and Revert must preserve saved grouping history")
        var resetDraft = LibraryDraft(groupedRecord)
        resetDraft[.title] = "Saved metadata"
        resetDraft.resetAlbumGrouping()
        var resetRefresh = record(groupedRecord.id, title: "Saved metadata", album: groupedRecord.album, albumArtist: groupedRecord.albumArtist)
        resetRefresh.albumGroup = groupedRecord.albumGroup
        resetRefresh.displayAlbum = groupedRecord.displayAlbum
        resetRefresh.displayAlbumArtist = groupedRecord.displayAlbumArtist
        resetDraft.markSaved(resetRefresh, artworkWasRead: false)
        let resetFields = try resetDraft.fieldsObject()
        let resetRetry = LibraryModel()
        resetRetry.showRemote(LanSession(id: 0, name: "Fixture betterPod"), records: [resetRefresh])
        resetRetry.tracks[0] = resetDraft
        check(resetDraft.changedFields.isEmpty && resetDraft.changeState(for: .title) == .saved && resetDraft.resetsAlbumGrouping &&
              resetDraft.albumGroupingChangeState == .modified && resetFields.count == 2 && resetFields["album"] as? String == groupedRecord.album &&
              resetFields["albumArtist"] as? String == groupedRecord.albumArtist && resetRetry.canSave,
              "Metadata saved before a failed private-group reset must retain a retryable grouping draft that forces both album fields")
        resetDraft.joinAlbum()
        resetDraft.markSaved(resetRefresh, artworkWasRead: false)
        check(resetDraft.joinsAlbum && resetDraft.resetsAlbumGrouping, "An unacknowledged join must retain a pending private-group reset too")
        resetRefresh.albumGroup = "remote-album:new-acknowledged-group"
        resetDraft.markSaved(resetRefresh, artworkWasRead: false, albumJoinWasSaved: true)
        check(!resetDraft.dirty && !resetDraft.joinsAlbum && !resetDraft.resetsAlbumGrouping && resetDraft.albumGroupingChangeState == .saved,
              "A confirmed join must take precedence over a pending reset even though its new logical group is nonempty")
        resetRetry.showRemote(LanSession(id: 0, name: "Fixture betterPod"), records: [resetRefresh])
        resetRetry.selection = [resetRefresh.id]
        resetRetry.set(.album, to: "Manually corrected album")
        check(resetRetry.tracks[0].resetsAlbumGrouping && resetRetry.tracks[0][.album] == "Manually corrected album",
              "A manual remote album identity edit must stage a private-group reset so a curation failure remains retryable")
        resetRetry.revertSelection()
        check(!resetRetry.tracks[0].resetsAlbumGrouping, "Revert must discard a manual identity edit's pending group reset")
        let legacySave = try JSONDecoder().decode(LibrarySaveResult.self, from: Data(#"{"saved":[],"failures":[],"error":null}"#.utf8))
        let joinedSave = try JSONDecoder().decode(LibrarySaveResult.self, from: Data(#"{"saved":["/remote/album/01.mp3"],"joined":["/remote/album/01.mp3"]}"#.utf8))
        check(legacySave.joined == nil && joinedSave.joined == [groupedRecord.id], "Grouping acknowledgments must decode while preserving older save replies")
        let model = LibraryModel()
        model.tracks = [a, b, readonly].map(LibraryDraft.init)
        model.selection = Set([a.id, b.id, readonly.id])
        check(model.isMixed(.title), "Mixed fields must be indicated")
        model.set(.albumArtist, to: "One album artist")
        check(model.dirtyCount == 2, "Bulk edits must skip read-only files")
        check(model.tracks[2][.albumArtist].isEmpty, "Read-only metadata must stay intact")
        model.selection = [readonly.id]
        check(model.commonValue(.title) == "Read only", "Read-only metadata must remain visible")
        model.selection = [a.id]
        model.revertSelection()
        check(!model.tracks[0].dirty && model.tracks[1].dirty, "Revert must only affect the selection")
        model.set(.year, to: "not a year")
        do {
            _ = try model.tracks[0].saveObject()
            preconditionFailure("Invalid numeric edits must not reach the writer")
        } catch is LibraryError {}
        model.revertSelection()
        model.set(.title, to: "")
        let clear = try model.tracks[0].saveObject()
        let fields = clear["fields"] as! [String: Any]
        check(fields["title"] as? String == "" && fields.count == 1, "Clearing must send one explicit sparse field")
        check(clear["cover"] == nil && clear["removeCover"] == nil, "Unedited artwork must not be sent")
        model.revertSelection()
        let ids = Set([a.id, b.id])
        check(model.orderedTargets(ids).map(\.id) == [b.id, a.id], "Complete tag numbering must establish order")
        let source = [
            AlbumLookupTrack(title: "New first", artist: "Singer one", disc: 1, track: 1, trackTotal: 2, duration: nil),
            AlbumLookupTrack(title: "New second", artist: "Singer two", disc: 1, track: 2, trackTotal: 2, duration: nil)
        ]
        let numbering = LibraryModel()
        let filenameFirst = record("/numbered/01. First.mp3", track: 2)
        let filenameSecond = record("/numbered/02 Second.mp3", track: 1)
        numbering.tracks = [filenameSecond, filenameFirst].map(LibraryDraft.init)
        let numberedIDs = Set(numbering.tracks.map(\.id))
        check(numbering.orderedTargets(numberedIDs).map(\.id) == [filenameFirst.id, filenameSecond.id],
              "Complete filename numbering must override incorrect reversed tags")
        let filenameDetails = AlbumLookupDetails(title: "Correct album", artist: "Artist", year: nil, genre: nil,
                                                tracks: source, cover: nil, coverWarning: nil)
        numbering.apply(filenameDetails, to: numberedIDs, includeTracks: true, includeArtwork: false)
        check(numbering.tracks.first(where: { $0.id == filenameFirst.id })?[.title] == "New first" &&
              numbering.tracks.first(where: { $0.id == filenameSecond.id })?[.title] == "New second",
              "Applying metadata must use the same filename order shown in the preview")
        let cd1 = record("/multidisc/CD1/01 First.mp3", track: 8)
        let cd2 = record("/multidisc/Disc2/01 Last.mp3", track: 1)
        let explicit = record("/multidisc/1-02 Middle.mp3", track: 9)
        numbering.tracks = [cd2, explicit, cd1].map(LibraryDraft.init)
        check(numbering.orderedTargets(Set(numbering.tracks.map(\.id))).map(\.id) == [cd1.id, explicit.id, cd2.id],
              "Filename disc prefixes and parent CD/Disc folders must establish coherent multidisc order")
        let unnumberedFirst = record("/natural/Zebra.mp3", track: 1)
        let unnumberedSecond = record("/natural/Alpha.mp3", track: 2)
        numbering.tracks = [unnumberedSecond, unnumberedFirst].map(LibraryDraft.init)
        check(numbering.orderedTargets(Set(numbering.tracks.map(\.id))).map(\.id) == [unnumberedFirst.id, unnumberedSecond.id],
              "Files without leading numbering must retain reliable tag order")
        numbering.selection = [unnumberedFirst.id]
        numbering.set(.track, to: "2")
        numbering.selection = [unnumberedSecond.id]
        numbering.set(.track, to: "1")
        check(numbering.orderedTargets(Set(numbering.tracks.map(\.id))).map(\.id) == [unnumberedSecond.id, unnumberedFirst.id],
              "Lookup ordering must honor corrected draft numbers when filenames do not provide reliable numbering")
        numbering.tracks = [record("/natural/Track 10.mp3"), record("/natural/Track 2.mp3")].map(LibraryDraft.init)
        check(numbering.orderedTargets(Set(numbering.tracks.map(\.id))).map { $0.original.fileName } == ["Track 2.mp3", "Track 10.mp3"],
              "Without complete filename or tag numbering, natural filename order is the fallback")
        let gapFirst = record("/gapped/03 First.mp3", track: 1)
        let gapSecond = record("/gapped/01 Second.mp3", track: 2)
        numbering.tracks = [gapSecond, gapFirst].map(LibraryDraft.init)
        check(numbering.orderedTargets(Set(numbering.tracks.map(\.id))).map(\.id) == [gapFirst.id, gapSecond.id],
              "Gapped filename numbering must not override a complete tag order")
        let details = AlbumLookupDetails(title: "Correct album", artist: "Various Artists", year: nil, genre: nil,
                                         tracks: source, cover: Data([1, 2, 3]), coverWarning: nil)
        model.apply(details, to: ids, includeTracks: true, includeArtwork: false)
        check(model.tracks[0][.title] == "New second" && model.tracks[1][.title] == "New first", "Lookup must use the preview order")
        check(model.tracks[0][.artist] == "Singer two", "Per-track artists must be used for compilations")
        check(model.tracks[0][.year] == "2020", "Missing source year must preserve the existing year")
        check(model.tracks[0].artwork == .keep, "Artwork opt-out must be honored")
        model.selection = ids
        model.revertSelection()
        model.apply(details, to: [a.id], includeTracks: true, includeArtwork: false)
        check(model.tracks[0][.title] == "Original", "Count mismatch must not overwrite track titles")
        check(model.tracks[0][.album] == "Correct album", "Album metadata can still apply on a count mismatch")
        model.revertSelection()
        model.apply(details, to: ids, includeTracks: true, includeArtwork: true, includeMetadata: false)
        check(model.tracks[0][.album] == "Album" && model.tracks[0][.title] == "Original" && model.tracks[0].artworkChangeState == .modified,
              "A cover-only lookup must never replace metadata")
        model.revertSelection()
        model.selection = []
        model.beginLookup()
        check(model.showingLookup && model.selection.count == 3 && model.lookupGroups.reduce(0, { $0 + $1.count }) == 3 && model.currentLookupGroup != nil,
              "Find Album without a selection must target the visible tracks")
        model.showingLookup = false

        let sorting = LibraryModel()
        let firstDisc = record("/sort/first.mp3", title: "Track 2", artist: "Performer Z", track: 1, album: "Album 2", albumArtist: "ALPHA", disc: 1)
        let nextTrack = record("/sort/next.mp3", title: "Track 10", artist: "Performer A", track: 2, album: "Album 2", albumArtist: "alpha", disc: 1)
        let secondDisc = record("/elsewhere/second-disc.mp3", title: "Zebra", artist: "Performer B", track: 1, album: "Album 2", albumArtist: "Alpha", disc: 2)
        let laterAlbum = record("/sort/later-album.mp3", title: "Track 1", artist: "Performer C", track: 1, album: "Album 10", albumArtist: "Alpha", disc: 1)
        let laterArtist = record("/sort/later-artist.mp3", title: "Track 1", artist: "Beta", track: 1, album: "Album 1", disc: 1)
        sorting.tracks = [laterArtist, secondDisc, laterAlbum, nextTrack, firstDisc].map(LibraryDraft.init)
        check(sorting.visibleTracks.map(\.id) == [firstDisc.id, nextTrack.id, secondDisc.id, laterAlbum.id, laterArtist.id],
              "Default sorting must group artist/album naturally, then disc and track across folders")
        sorting.sortOrder = [KeyPathComparator(\LibraryDraft.sortTitle)]
        check(sorting.visibleTracks.map(\.sortTitle) == ["Track 1", "Track 1", "Track 2", "Track 10", "Zebra"], "Header-created string comparators must use natural, case-insensitive order")
        sorting.sortOrder[0].order = .reverse
        check(sorting.visibleTracks.map(\.sortTitle) == ["Zebra", "Track 10", "Track 2", "Track 1", "Track 1"], "Reversing a header comparator must reverse its primary order")
        sorting.sortOrder = [KeyPathComparator(\LibraryDraft.sortTrack, order: .reverse)]
        check(sorting.visibleTracks.first?.id == nextTrack.id, "Numeric track sorting must use numeric values")
        sorting.sortOrder = [KeyPathComparator(\LibraryDraft.sortDisc), KeyPathComparator(\LibraryDraft.sortTrack)]
        check(sorting.visibleTracks.last?.id == secondDisc.id, "Disc sorting must preserve the complete multidisc order")
        sorting.tracks[0].artwork = .replace(coverData)
        sorting.sortOrder = [KeyPathComparator(\LibraryDraft.sortArtwork, order: .reverse)]
        check(sorting.visibleTracks.first?.id == laterArtist.id, "Artwork header sorting must use cover presence")
        sorting.tracks.append(provenance)
        sorting.sortOrder = [KeyPathComparator(\LibraryDraft.sortChanges, order: .reverse)]
        check(sorting.visibleTracks.prefix(2).map(\.id) == [laterArtist.id, provenance.id], "Change sorting must distinguish pending, saved and unchanged tracks")

        let regrouping = LibraryModel()
        let cleanAlbum = record("/one/01.mp3", artist: "Singer One", album: "  Café   Sessions ", albumArtist: " The   Band ")
        let alternateAlbum = record("/two/02.mp3", artist: "Singer Two", album: "Other Album", albumArtist: "the band")
        regrouping.tracks = [cleanAlbum, alternateAlbum].map(LibraryDraft.init)
        check(regrouping.albums.count == 2, "Different album metadata starts as distinct groups")
        regrouping.selection = [alternateAlbum.id]
        regrouping.albumFilter = regrouping.albums.first(where: { $0.title == "Other Album" })!.id
        regrouping.set(.album, to: "cafe sessions")
        check(regrouping.albums.count == 1 && regrouping.albums[0].count == 2 && regrouping.visibleTracks.count == 2,
              "Current draft tags must immediately join albums across folders despite case, whitespace and diacritics")
        check(regrouping.selection == [alternateAlbum.id], "Following a regrouped filter must not expand the track selection")
        regrouping.revertSelection()
        check(regrouping.albums.count == 2 && regrouping.visibleTracks.count == 1, "Reverting an album edit must restore its group and active filter")
        let normalized = LibraryModel()
        normalized.tracks = [cleanAlbum, record("/three/03.mp3", artist: "Another Singer", album: "cafe sessions", albumArtist: "the band")].map(LibraryDraft.init)
        check(normalized.albums.count == 1, "Identical normalized tags in different physical folders represent one local album")
        let artistGrouping = LibraryModel()
        let hitsA = record("/hits/a/01.mp3", artist: "Artist A", album: "Greatest Hits")
        let hitsB = record("/hits/b/01.mp3", artist: "Artist B", album: "Greatest Hits")
        let hitsAElsewhere = record("/elsewhere/01.mp3", artist: "  ARTIST   A ", album: "greatest hits")
        artistGrouping.tracks = [hitsA, hitsB, hitsAElsewhere].map(LibraryDraft.init)
        check(artistGrouping.albums.count == 2 && artistGrouping.albums.first(where: { $0.artist == "Artist A" || $0.artist == "ARTIST A" })?.count == 2,
              "Missing album artists must keep unrelated same-title albums separate while joining normalized same-performer albums across folders")
        artistGrouping.selection = [hitsB.id]
        artistGrouping.albumFilter = artistGrouping.albums.first(where: { $0.artist == "Artist B" })!.id
        artistGrouping.set(.artist, to: "Artist A")
        check(artistGrouping.albums.count == 1 && artistGrouping.visibleTracks.count == 3 && artistGrouping.selection == [hitsB.id],
              "Correcting the inferred performer must invalidate grouping and follow the edited album filter")
        let sameFolderCompilation = LibraryModel()
        sameFolderCompilation.tracks = [record("/various/CD1/01.mp3", artist: "Singer A", album: "A Compilation"),
                                        record("/various/Disc2/01.mp3", artist: "Singer B", album: "A Compilation")].map(LibraryDraft.init)
        check(sameFolderCompilation.albums.count == 1 && sameFolderCompilation.albums[0].artist == "Various Artists" && !sameFolderCompilation.hasChanges,
              "A blank-album-artist compilation must remain together across disc subfolders without inventing pending tags")
        let untagged = LibraryModel()
        let firstUntagged = record("/untagged/First Album/01.mp3", title: "Zebra", album: "")
        let secondUntagged = record("/untagged/Second Album/01.mp3", title: "Alpha", album: "Unknown Album")
        untagged.tracks = [secondUntagged, firstUntagged].map(LibraryDraft.init)
        check(untagged.albums.map(\.title) == ["First Album", "Second Album"] && untagged.visibleTracks.map(\.id) == [firstUntagged.id, secondUntagged.id],
              "Absent or placeholder album tags must use distinct folder identities and labels before track-title sorting")
        let unknownDiscOne = record("/untagged/Boxed Album/CD1/01.mp3", album: "Unknown")
        let unknownDiscTwo = record("/untagged/Boxed Album/Disc2/01.mp3", album: "Untitled")
        untagged.tracks += [unknownDiscOne, unknownDiscTwo].map(LibraryDraft.init)
        check(untagged.albums.count == 3 && untagged.albums.first(where: { $0.title == "Boxed Album" })?.count == 2,
              "CD1 and Disc2 folders of one unnamed album must share the parent album identity")
        untagged.selection = [firstUntagged.id]
        let unnamedTarget = untagged.albums.first(where: { $0.title == "Second Album" })!.id
        untagged.mergeSelection(into: unnamedTarget)
        check(!untagged.hasChanges && untagged.error?.contains("Identify or name") == true,
              "Merging into an unnamed target must not copy blank tags and falsely claim a merge")
        untagged.set(.album, to: "Identified Album")
        untagged.selection = [secondUntagged.id]
        let namedTarget = untagged.albums.first(where: { $0.title == "Identified Album" })!.id
        untagged.mergeSelection(into: namedTarget)
        check(untagged.albums.first(where: { $0.title == "Identified Album" })?.count == 2 && untagged.selectedTracks[0][.album] == "Identified Album",
              "An unnamed source can merge into a known target using pending album tags")

        let albumQueue = LibraryModel()
        let albumOneA = record("/select/a1.mp3", album: "Album One")
        let albumOneB = record("/select/a2.mp3", writable: false, album: "Album One")
        let albumTwo = record("/select/b.mp3", album: "Album Two")
        let albumThree = record("/select/c.mp3", writable: false, album: "Album Three")
        albumQueue.tracks = [albumTwo, albumOneA, albumThree, albumOneB].map(LibraryDraft.init)
        albumQueue.selection = Set(albumQueue.tracks.map(\.id))
        albumQueue.beginLookup()
        check(albumQueue.lookupGroups == [[albumOneA.id, albumOneB.id], [albumTwo.id]] && albumQueue.currentLookupGroup == [albumOneA.id, albumOneB.id],
              "Find Album takes one album at a time in table order and leaves out albums without an editable track")
        albumQueue.nextLookup()
        check(albumQueue.showingLookup && albumQueue.currentLookupGroup == [albumTwo.id], "Apply or Skip moves to the next album")
        albumQueue.nextLookup()
        check(!albumQueue.showingLookup && albumQueue.currentLookupGroup == nil, "Find Album closes after the last album")
        albumQueue.busy = true
        albumQueue.beginLookup()
        check(!albumQueue.showingLookup, "Find Album cannot start during an active operation")
        albumQueue.busy = false

        let changesScope = LibraryModel()
        let changedA = record("/changes/one/a.mp3", album: "One")
        let cleanA = record("/changes/one/b.mp3", album: "One")
        let changedB = record("/changes/two/a.mp3", album: "Two")
        changesScope.tracks = [changedA, cleanA, changedB].map(LibraryDraft.init)
        changesScope.selection = [changedA.id, changedB.id]
        changesScope.set(.title, to: "Edited")
        changesScope.selection = Set(changesScope.tracks.map(\.id))
        changesScope.changesOnly = true
        check(changesScope.selection == [changedA.id, changedB.id] && changesScope.visibleTracks.count == 2,
              "Enabling Changes must remove hidden unchanged rows from both the visible scope and selection")
        let changesAlbum = changesScope.albums.first(where: { $0.title == "One" })!.id
        changesScope.albumFilter = changesAlbum
        changesScope.filterChanged()
        check(changesScope.selection == [changedA.id], "Changing albums under Changes must select only visible changed tracks")
        changesScope.selection = []
        changesScope.beginLookup()
        check(changesScope.showingLookup && changesScope.selection == [changedA.id] && changesScope.dirtyCount == 2 && changesScope.canSave,
              "Find Album without a selection must target only the visible changed album, while Save retains all dirty drafts")
        changesScope.showingLookup = false
        changesScope.selection = [cleanA.id]
        changesScope.beginLookup()
        check(changesScope.showingLookup && changesScope.selection == [changedA.id],
              "Lookup must clamp hidden selections and fall back to the actual visible changed rows")
        changesScope.showingLookup = false
        changesScope.changesOnly = false
        changesScope.selection = [changedA.id, cleanA.id]
        changesScope.changesOnly = true
        changesScope.revertSelection()
        check(changesScope.visibleTracks.isEmpty && changesScope.selection.isEmpty && !changesScope.canLookup,
              "Reverting the final change must remove its row and selection from Changes, preventing hidden inspector targets")
        changesScope.beginLookup()
        check(!changesScope.showingLookup && changesScope.selection.isEmpty, "Lookup must remain closed when the filtered album has no changed rows")
        changesScope.albumFilter = ""
        changesScope.filterChanged()
        check(changesScope.selection == [changedB.id], "Changing back to all albums must still select only changed visible tracks")
        changesScope.clearLibrary()
        check(!changesScope.changesOnly, "Clearing the library must reset Changes")
        changesScope.undoRemoval()
        check(changesScope.changesOnly && changesScope.selection == [changedB.id] && changesScope.visibleTracks.map(\.id) == [changedB.id],
              "Undoing a clear must restore the Changes filter and its visible selection")

        let merging = LibraryModel()
        let mergeTarget = record("/merge/target/01.mp3", title: "Target Song", artist: "Target Performer", track: 1, cover: coverData,
                                 album: "Joined Album", albumArtist: "Album Artist", disc: 1, genre: "Rock")
        let mergeSource = record("/merge/source/02.mp3", title: "Source Song", artist: "Guest Performer", track: 2, cover: coverData,
                                 album: "Split Album", albumArtist: "Wrong Album Artist", disc: 2, genre: "Jazz")
        let mergeReadonly = record("/merge/readonly.wav", title: "Read-only Song", writable: false, album: "Read-only Album")
        merging.tracks = [mergeSource, mergeReadonly, mergeTarget].map(LibraryDraft.init)
        merging.selection = [mergeSource.id]
        merging.albumFilter = merging.albums.first(where: { $0.title == "Split Album" })!.id
        let targetID = merging.albums.first(where: { $0.title == "Joined Album" })!.id
        check(merging.canMergeSelection && !merging.mergeTargets.contains(where: { $0.title == "Split Album" }), "A single-source merge must offer other loaded albums")
        merging.error = "An earlier error"
        merging.beginMerge()
        check(merging.showingMerge && merging.error == nil, "A valid merge command must open its shared sheet and clear stale errors")
        merging.mergeSelection(into: targetID)
        let merged = merging.tracks.first(where: { $0.id == mergeSource.id })!
        check(merged[.album] == "Joined Album" && merged[.albumArtist] == "Album Artist" && merged.changedFields == [.album, .albumArtist], "Merge must change only the target album identity fields")
        check(merged[.artist] == "Guest Performer" && merged[.title] == "Source Song" && merged[.genre] == "Jazz" && merged[.disc] == "2" && merged[.track] == "2" && merged.cover == coverData && merged.artworkChangeState == .unchanged,
              "Merge must preserve performers, titles, genre, numbering and cover art")
        check(merging.albums.count == 2 && merging.visibleTracks.count == 2 && merging.selection == [mergeSource.id] && !merging.showingMerge,
              "Merge must regroup immediately, follow the target filter and preserve the selection")
        merging.revertSelection()
        check(merging.albums.count == 3 && merging.selectedTracks[0][.album] == "Split Album", "Merged drafts must support the regular Revert command")
        merging.selection = [mergeSource.id, mergeTarget.id, mergeReadonly.id]
        merging.mergeSelection(into: targetID)
        check(!merging.tracks.first(where: { $0.id == mergeTarget.id })!.dirty && merging.tracks.first(where: { $0.id == mergeReadonly.id })![.album] == "Read-only Album",
              "Source/target overlap is allowed, while read-only files remain untouched")
        let compilation = LibraryModel()
        let compilationTarget = record("/compilation/a.mp3", artist: "Performer A", album: "Compilation")
        let compilationSource = record("/compilation/b.mp3", artist: "Performer B", album: "Other")
        compilation.tracks = [compilationTarget, compilationSource].map(LibraryDraft.init)
        compilation.selection = [compilationSource.id]
        compilation.albumFilter = compilation.albums.first(where: { $0.title == "Other" })!.id
        compilation.mergeSelection(into: compilation.albums.first(where: { $0.title == "Compilation" })!.id)
        check(compilation.albums.count == 1 && compilation.tracks.allSatisfy { $0[.albumArtist] == "Performer A" } && compilation.visibleTracks.count == 2 &&
              compilation.tracks[0][.artist] == "Performer A" && compilation.tracks[1][.artist] == "Performer B",
              "A blank-AA destination must stamp its inferred artist on both destination and incoming drafts, follow the new identity and preserve performers")
        let crossFolderCompilation = LibraryModel()
        let variousA = record("/collection/01.mp3", artist: "Singer A", album: "Collection")
        let variousB = record("/collection/02.mp3", artist: "Singer B", album: "Collection")
        let incomingVarious = record("/incoming/03.mp3", artist: "Singer C", album: "Split Collection")
        crossFolderCompilation.tracks = [variousA, variousB, incomingVarious].map(LibraryDraft.init)
        crossFolderCompilation.selection = [incomingVarious.id]
        crossFolderCompilation.mergeSelection(into: crossFolderCompilation.albums.first(where: { $0.title == "Collection" })!.id)
        check(crossFolderCompilation.albums.count == 1 && crossFolderCompilation.tracks.allSatisfy { $0[.albumArtist] == "Various Artists" },
              "Merging into a compilation must harmonize destination and incoming album artists so separate physical folders converge")
        let readOnlyDestination = LibraryModel()
        let lockedTarget = record("/locked/a.wav", artist: "Locked Artist", writable: false, album: "Locked Album")
        readOnlyDestination.tracks = [lockedTarget, incomingVarious].map(LibraryDraft.init)
        readOnlyDestination.selection = [incomingVarious.id]
        readOnlyDestination.showingMerge = true
        readOnlyDestination.mergeSelection(into: readOnlyDestination.albums.first(where: { $0.title == "Locked Album" })!.id)
        check(!readOnlyDestination.hasChanges && readOnlyDestination.showingMerge && readOnlyDestination.error?.contains("read-only") == true,
              "A merge requiring changes to a read-only destination must fail atomically and remain reviewable")

        let found = LibraryModel()
        let partialThree = record("/partial/03.mp3", title: "Old Three", track: 3, album: "Old Album")
        let partialEight = record("/split-partial/08.mp3", title: "Old Eight", track: 8, album: "Old Album")
        let untouchedPartial = record("/partial/outside.mp3", title: "Untouched")
        found.tracks = [partialThree, partialEight, untouchedPartial, readonly].map(LibraryDraft.init)
        let fullTrackList = (1...10).map { AlbumLookupTrack(title: "Source \($0)", artist: "Singer \($0)", disc: 1, track: $0, trackTotal: 10, duration: nil) }
        let matchedDetails = AlbumLookupDetails(title: "Matched Album", artist: "Album Artist", year: 2024, genre: "Rock", tracks: fullTrackList, cover: coverData, coverWarning: nil)
        found.apply(matchedDetails, to: [partialThree.id, partialEight.id, readonly.id], includeTracks: true, includeArtwork: true)
        check(found.tracks[0][.title] == "Old Three" && found.tracks[1][.title] == "Old Eight" && found.tracks[0][.track] == "3",
              "A partial selection keeps its track names and numbers: only a complete track list is paired")
        check(found.tracks[2][.title] == "Untouched" && found.tracks[2][.album] == "Album" && found.tracks[3][.title] == "Read only",
              "Find Album must ignore unselected and read-only tracks")
        check(found.tracks[0].artworkChangeState == .modified && found.tracks[1][.albumArtist] == "Album Artist" && found.tracks[1][.artist] == "Artist",
              "Find Album keeps album details separate from individual performers and marks the new cover")
        check(found.albums.first(where: { $0.title == "Matched Album" })?.count == 2,
              "Find Album joins tracks from different folders into their identified album")
        let blocked = AlbumLookupDetails(title: "Must Not Apply", artist: "", year: nil, genre: nil, tracks: [], cover: nil, coverWarning: nil)
        found.busy = true
        found.apply(blocked, to: [partialThree.id], includeTracks: false, includeArtwork: false)
        check(found.tracks[0][.album] == "Matched Album" && !found.canLookup, "Find Album cannot apply or start while another model operation is active")
        found.busy = false
        found.remoteNeedsReload = true
        found.apply(blocked, to: [partialThree.id], includeTracks: false, includeArtwork: false)
        check(found.tracks[0][.album] == "Matched Album" && !found.canLookup, "Find Album must respect a source that requires reload")
        found.remoteNeedsReload = false
        found.selection = []
        check(found.canLookup, "Find Album can use visible editable tracks when no rows are selected")
        found.beginLookup()
        check(found.showingLookup && found.selection.count == 4 && !found.lookupGroups.isEmpty, "Find Album must select visible tracks and open its sheet")
        found.clearLibrary()
        check(!found.showingMerge && !found.showingLookup, "Clearing the library must close any library task sheet")

        // Exercise the actual Swift/C/Go JSON bridge, including large file
        // timestamps, sparse writes and a stale second save.
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("bp-library-bridge-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appendingPathComponent("01 bridge.mp3")
        let audio = Data([0xff, 0xfb, 0x90, 0x64]) + Data("Audio payload preserved by the bridge test".utf8)
        try audio.write(to: file)
        let loaded = try LibraryEngine.read([directory])
        check(loaded.error == nil && loaded.tracks?.count == 1, "Folder import must decode through the actual C bridge")
        var draft = LibraryDraft(loaded.tracks![0])
        draft[.title] = "Bridge title ✓"
        draft[.album] = "Bridge album"
        let request = try JSONSerialization.data(withJSONObject: ["tracks": [try draft.saveObject()]])
        let saved = try LibraryEngine.save(request)
        check(saved.saved == [file.path] && saved.failures?.isEmpty == true, "Bridge edits must return a successful file result")
        let reread = try LibraryEngine.read([file])
        check(reread.tracks?.first?.title == "Bridge title ✓" && reread.tracks?.first?.album == "Bridge album", "Saved Unicode tags must read back through the bridge")
        let written = try Data(contentsOf: file)
        check(written.suffix(audio.count) == audio, "Bridge edits must preserve the audio payload")
        let conflict = try LibraryEngine.save(request)
        check(conflict.saved?.isEmpty == true && conflict.failures?.count == 1, "A stale bridge request must preserve the changed file")

        let removal = LibraryModel()
        removal.tracks = [LibraryDraft(loaded.tracks![0]), provenance]
        removal.location = "Album folder"
        removal.lookupFolderName = "Album source"
        removal.selection = [file.path]
        removal.set(.title, to: "Keep this unsaved title")
        let nativeUndo = UndoManager()
        nativeUndo.groupsByEvent = false
        removal.removeSelection(undoManager: nativeUndo)
        check(removal.tracks.count == 1 && nativeUndo.canUndo, "Removing selected tracks must register native Undo")
        let typingOwner = NSObject()
        var typingWasUndone = false
        nativeUndo.beginUndoGrouping()
        nativeUndo.registerUndo(withTarget: typingOwner) { _ in typingWasUndone = true }
        nativeUndo.setActionName("Typing")
        nativeUndo.endUndoGrouping()
        check(!removal.canUndoRemoval, "The explicit removal Undo must not target a later unrelated text edit")
        removal.undoRemoval()
        check(!typingWasUndone, "Undo Library Removal must never undo unrelated typing")
        nativeUndo.undo()
        check(typingWasUndone && removal.canUndoRemoval, "Normal native Undo can restore the removal action to the top")
        removal.selection = [a.id]
        removal.set(.artist, to: "A later surviving edit")
        nativeUndo.undo()
        check(removal.tracks.count == 2 && removal.selectedTracks.first?[.title] == "Keep this unsaved title",
              "Undo must restore removed drafts and their selection")
        check(removal.tracks.first(where: { $0.id == a.id })?[.artist] == "A later surviving edit", "Undoing a removal must retain newer edits on surviving rows")
        check(removal.tracks.first(where: { $0.id == a.id })?.changeState(for: .title) == .saved, "Removing and restoring rows must preserve saved field history")
        nativeUndo.redo()
        check(removal.tracks.count == 1 && removal.tracks[0][.artist] == "A later surviving edit", "Redo must remove the restored rows without rewinding surviving edits")
        removal.selection = [a.id]
        removal.clearLibrary()
        check(removal.tracks.isEmpty && removal.location.isEmpty && removal.lookupFolderName.isEmpty && removal.canUndoRemoval,
              "Clear Library must reset the loaded folder and retain a fallback Undo")
        removal.undoRemoval()
        check(removal.tracks.count == 1 && removal.location == "Album folder" && removal.lookupFolderName == "Album source" && removal.selection == [a.id],
              "Fallback Undo must restore the cleared folder context and draft")
        removal.busy = true
        removal.clearLibrary()
        removal.removeSelection()
        check(removal.tracks.count == 1, "An active operation must prevent removal")
        removal.busy = false
        let afterRemoval = try Data(contentsOf: file)
        check(afterRemoval == written, "Clear/remove/Undo must never delete or rewrite the source file")

        let joinSession = LanSession(id: 0, name: "Fixture betterPod", supportsAlbumJoining: true)
        let remote = LibraryModel()
        var remoteRecord = reread.tracks![0]
        remoteRecord.displayAlbum = "Player grouping"
        remoteRecord.displayAlbumArtist = "Player album artist"
        remoteRecord.albumGroup = "private-group"
        remote.showRemote(joinSession, records: [remoteRecord])
        check(remote.isRemote && remote.selection.isEmpty, "A device library must keep its own source and require an explicit selection")
        check(remote.albums.first?.title == "Player grouping", "The device album filter must reflect its displayed grouping")
        var distinctRecord = record("/different-player-album/song.mp3")
        distinctRecord.displayAlbum = remoteRecord.displayAlbum
        distinctRecord.displayAlbumArtist = remoteRecord.displayAlbumArtist
        distinctRecord.albumGroup = "another-private-group"
        distinctRecord.displayAlbumID = "different-player-album"
        remoteRecord.displayAlbumID = "first-player-album"
        remote.showRemote(joinSession, records: [remoteRecord, distinctRecord])
        check(remote.albums.count == 2, "Explicit private player groups must stay distinct until a grouping action")
        remote.selection = [remoteRecord.id, distinctRecord.id]
        check(remote.canMergeSelection && remote.canLookup, "A capable connected player must support the shared merge and Find Album tools")
        remote.beginMerge()
        check(remote.showingMerge, "Remote Merge must open the same review sheet")
        remote.beginLookup()
        check(remote.showingLookup && !remote.showingMerge && !remote.hasChanges && remote.lookupGroups.count == 2,
              "Opening remote Find Album must not mutate drafts, and keeps private player albums apart")
        remote.showingLookup = false
        remote.set(.album, to: "Shared Draft Album")
        remote.set(.albumArtist, to: "Shared Album Artist")
        check(remote.albums.count == 1, "Explicit remote album edits may preview their current common tags")
        remote.revertSelection()
        check(remote.albums.count == 2, "Reverting remote album edits must restore the device's private identities")

        let remoteRules = LibraryModel()
        var logicalOne = record("/device/one/01.mp3", title: "Track 2", artist: "Singer A", track: 1, album: "Physical One", albumArtist: "Physical Artist")
        logicalOne.displayAlbum = "  Café   Sessions "
        logicalOne.displayAlbumArtist = " The   Band "
        logicalOne.displayAlbumID = "/device/one|café sessions"
        var logicalTwo = record("/device/two/02.mp3", title: "Track 10", artist: "Singer B", track: 2, album: "Physical Two", albumArtist: "Physical Artist")
        logicalTwo.displayAlbum = "cafe sessions"
        logicalTwo.displayAlbumArtist = "the band"
        logicalTwo.displayAlbumID = "/device/two|cafe sessions"
        remoteRules.showRemote(joinSession, records: [logicalTwo, logicalOne])
        check(remoteRules.albums.count == 1 && remoteRules.albums[0].count == 2 && remoteRules.visibleTracks.map(\.id) == [logicalOne.id, logicalTwo.id],
              "Ordinary remote folder IDs must use the same normalized logical tags and smart ordering as local albums")
        var remoteUnknownOne = record("/device/Unknown One/CD1/01.mp3", album: "")
        remoteUnknownOne.displayAlbumID = "/device/Unknown One|"
        var remoteUnknownTwo = record("/device/Unknown Two/01.mp3", album: "Unknown")
        remoteUnknownTwo.displayAlbumID = "/device/Unknown Two|unknown"
        var remoteUnknownDisc = record("/device/Unknown One/Disc2/01.mp3", album: "Unknown Album")
        remoteUnknownDisc.displayAlbumID = "/device/Unknown One|unknown album"
        remoteRules.showRemote(joinSession, records: [remoteUnknownOne, remoteUnknownTwo, remoteUnknownDisc])
        check(remoteRules.albums.count == 2 && remoteRules.albums.first(where: { $0.title == "Unknown One" })?.count == 2,
              "Unnamed remote albums must share the local folder fallback and multidisc rules")

        var privateTarget = record("/device/target/01.mp3", artist: "Singer A", album: "Joined Album", albumArtist: "Album Artist")
        privateTarget.albumGroup = "target-private"
        privateTarget.displayAlbum = privateTarget.album
        privateTarget.displayAlbumArtist = privateTarget.albumArtist
        var privateSource = record("/device/source/02.mp3", artist: "Singer B", album: "Joined Album", albumArtist: "Album Artist")
        privateSource.albumGroup = "source-private"
        privateSource.displayAlbum = privateSource.album
        privateSource.displayAlbumArtist = privateSource.albumArtist
        remoteRules.showRemote(joinSession, records: [privateTarget, privateSource])
        remoteRules.selection = [privateSource.id]
        remoteRules.albumFilter = remoteRules.albums.first(where: { $0.id == "group:source-private" })!.id
        remoteRules.mergeSelection(into: "group:target-private")
        check(remoteRules.albums.count == 1 && remoteRules.visibleTracks.count == 2 &&
              remoteRules.tracks.allSatisfy { $0.joinsAlbum && $0.dirty && $0.changedFields.isEmpty },
              "Remote Merge must assign both private destination and incoming tracks to the common album even when tags are already identical")
        let remoteMergeObjects = try remoteRules.tracks.map { try $0.saveObject() }
        check(remoteMergeObjects.allSatisfy { $0["joinAlbum"] as? Bool == true && ($0["fields"] as? [String: Any])?.count == 2 && $0["cover"] == nil },
              "Remote grouping must use the existing sparse tag protocol plus explicit joins without touching unknown covers")
        remoteRules.selection = [privateTarget.id, privateSource.id]
        remoteRules.revertSelection()
        check(remoteRules.albums.count == 2 && !remoteRules.hasChanges, "Revert must restore both original private groups before saving")
        let protectedTarget = record("/device/protected.wav", artist: "Singer A", writable: false, album: "Protected Album", albumArtist: "Album Artist")
        remoteRules.showRemote(joinSession, records: [protectedTarget, privateSource])
        remoteRules.selection = [privateSource.id]
        remoteRules.mergeSelection(into: remoteRules.albums.first(where: { $0.title == "Protected Album" })!.id)
        check(!remoteRules.hasChanges && remoteRules.error?.contains("read-only") == true,
              "A remote destination that cannot receive the required group assignment must refuse the complete merge")

        let remotePartial = LibraryModel()
        var remoteThree = record("/device/partial/03.mp3", title: "Old Three", track: 3, album: "Old Album")
        remoteThree.albumGroup = "old-three"
        var remoteEight = record("/device/split/08.mp3", title: "Old Eight", track: 8, album: "Old Album")
        remoteEight.albumGroup = "old-eight"
        let playingTrack = record("/device/playing.mp3", title: "Playing", writable: false, album: "Old Album")
        let unmatchedTrack = record("/device/unmatched.mp3", title: "Unmatched", album: "Old Album")
        remotePartial.showRemote(joinSession, records: [remoteThree, remoteEight, playingTrack, unmatchedTrack])
        remotePartial.selection = []
        remotePartial.beginLookup()
        check(remotePartial.showingLookup && remotePartial.selection.count == 4 && remotePartial.lookupGroups.count == 3,
              "Remote Find Album uses the same visible scope, one player album at a time")
        remotePartial.showingLookup = false
        remotePartial.apply(matchedDetails, to: [remoteThree.id, remoteEight.id, playingTrack.id], includeTracks: false, includeArtwork: true)
        check(remotePartial.tracks[0][.title] == "Old Three" && remotePartial.tracks[0].joinsAlbum && remotePartial.tracks[1].joinsAlbum &&
              !remotePartial.tracks[2].dirty && !remotePartial.tracks[3].dirty,
              "Remote Find Album stages joins only for selected writable rows and skips the protected playing track")
        check(remotePartial.albums.first(where: { $0.title == "Matched Album" })?.count == 2 && remotePartial.tracks[0].artworkChangeState == .modified,
              "Remote Find Album previews a common album and tracks the cover change separately")

        let legacyRemote = LibraryModel()
        legacyRemote.showRemote(LanSession(id: 0, name: "Older Rust"), records: [privateTarget, privateSource])
        legacyRemote.selection = [privateSource.id]
        check(!legacyRemote.canJoinAlbums && legacyRemote.canLookup && !legacyRemote.canMergeSelection && legacyRemote.canEdit,
              "Older consoles must keep ordinary editing and Find Album while hiding unsupported grouping tools")
        legacyRemote.mergeSelection(into: "group:target-private")
        check(!legacyRemote.hasChanges, "Unsupported console builds must never receive join requests")
        legacyRemote.apply(matchedDetails, to: [privateSource.id], includeTracks: false, includeArtwork: false)
        check(legacyRemote.hasChanges && !legacyRemote.tracks.contains(where: \.joinsAlbum), "Find Album edits tags on older consoles without join requests")
        legacyRemote.revertSelection()
        legacyRemote.set(.title, to: "Compatible metadata edit")
        check(legacyRemote.hasChanges && !legacyRemote.tracks[1].joinsAlbum, "Plain metadata edits must remain compatible with older consoles")

        remote.showRemote(joinSession, records: [remoteRecord])
        remote.selection = [remoteRecord.id]
        remote.set(.title, to: "Unsaved remote draft")
        check(remote.canSave, "Connected remote drafts can be saved")
        remote.remoteDisconnected()
        check(!remote.canSave && remote.isRemote && remote.hasChanges, "Disconnect must keep the remote draft and disable writes")
        remote.remoteNeedsReload = false
        remote.set(.title, to: "Must not apply")
        remote.setArtwork(.remove)
        remote.beginLookup()
        remote.apply(matchedDetails, to: [remoteRecord.id], includeTracks: true, includeArtwork: true)
        check(!remote.canEdit && !remote.canSave && !remote.canLookup && !remote.showingLookup && remote.tracks[0][.title] == "Unsaved remote draft" && remote.tracks[0].artwork == .keep,
              "A missing remote session must block every edit path even if the reload flag has not yet updated")
        remote.remoteNeedsReload = true
        remote.open([directory])
        remote.clearLibrary()
        remote.removeSelection()
        remote.save()
        check(remote.tracks[0][.title] == "Unsaved remote draft", "Local file opens must not replace a remote source")
        let afterDisconnect = try Data(contentsOf: file)
        check(afterDisconnect == written, "A disconnected device editor must never fall back to writing local paths")
        remote.discardRemoteDrafts()
        check(!remote.hasChanges && remote.isRemote, "Explicit discard must clear the remote draft without changing its source")

        let batchDirectory = directory.appendingPathComponent("batch")
        try FileManager.default.createDirectory(at: batchDirectory, withIntermediateDirectories: true)
        let batchFiles = (1...51).map { batchDirectory.appendingPathComponent(String(format: "%02d Batch.mp3", $0)) }
        for file in batchFiles { try audio.write(to: file) }
        var drafts = try LibraryEngine.read([batchDirectory]).tracks!.map(LibraryDraft.init)
        for index in drafts.indices { drafts[index][.title] = "Batch title \(index)" }
        drafts[drafts.count - 1][.year] = "invalid"
        do {
            _ = try LibraryBatchWriter.save(drafts, session: nil)
            preconditionFailure("An invalid field in the last batch must fail before the first file is saved")
        } catch is LibraryError {}
        let untouched = try Data(contentsOf: batchFiles[0])
        check(untouched == audio, "Validation must finish for every draft before any batch writes")
        drafts[drafts.count - 1][.year] = "2026"
        let batchResult = try LibraryBatchWriter.save(drafts, session: nil)
        check(batchResult.result.saved?.count == 51 && batchResult.result.failures?.isEmpty == true && !batchResult.requiresReload,
              "A bulk edit larger than one batch must save every track")
        let last = try LibraryEngine.read([batchFiles.last!]).tracks!.first!
        check(last.title == "Batch title 50" && last.year == 2026, "Later batches must contain their own edits")

        let historyFile = directory.appendingPathComponent("History.mp3")
        try audio.write(to: historyFile)
        let historyRecord = try LibraryEngine.read([historyFile]).tracks!.first!
        let history = LibraryModel()
        history.tracks = [LibraryDraft(historyRecord)]
        history.selection = [historyRecord.id]
        history.set(.title, to: "Saved with history")
        history.setArtwork(.replace(coverData))
        history.save()
        for _ in 0..<200 where history.busy { try await Task.sleep(nanoseconds: 10_000_000) }
        check(!history.busy && history.error == nil && !history.hasChanges, "The real model save must complete successfully")
        check(history.tracks[0].changeState(for: .title) == .saved && history.tracks[0].artworkChangeState == .saved,
              "A real save refresh must retain metadata and artwork marks")
        history.set(.title, to: "Pending again")
        history.revertSelection()
        check(history.tracks[0][.title] == "Saved with history" && history.tracks[0].changeState(for: .title) == .saved && history.tracks[0].artworkChangeState == .saved,
              "Reverting later edits must keep a real save's history")
        history.set(.artist, to: "A later artist edit")
        history.save()
        for _ in 0..<200 where history.busy { try await Task.sleep(nanoseconds: 10_000_000) }
        check(!history.busy && history.error == nil && history.tracks[0].artworkChangeState == .saved && history.tracks[0].cover != nil,
              "A later metadata-only save must preserve the saved cover and its change mark")
        print("Library model tests passed")
    }
}
