import Foundation

private final class MagicFixtureProtocol: URLProtocol {
    static var names = ["First Song", "Second Song", "Finale Song"]
    static var editions = [(id: 500, year: 2001)]
    static var coverRequests = 0
    static var includeCover = false
    static let png = Data(base64Encoded: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")!
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        if request.url?.host == "is1-ssl.mzstatic.com" {
            Self.coverRequests += 1
            let response = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: "HTTP/1.1", headerFields: nil)!
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: Self.png)
            client?.urlProtocolDidFinishLoading(self)
            return
        }
        let code: Int
        let object: [String: Any]
        switch request.url?.host {
        case "musicbrainz.org": code = 503; object = [:]
        case "api.deezer.com": code = 200; object = ["data": []]
        case "itunes.apple.com":
            code = 200
            func album(_ id: Int, _ year: Int) -> [String: Any] {
                var row: [String: Any] = ["wrapperType": "collection", "collectionId": id, "collectionName": "Album", "artistName": "Artist", "trackCount": Self.names.count, "releaseDate": "\(year)-01-01"]
                if Self.includeCover { row["artworkUrl100"] = "https://is1-ssl.mzstatic.com/cover/100x100bb.jpg" }
                return row
            }
            if request.url?.path == "/lookup" {
                let id = URLComponents(url: request.url!, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "id" }?.value.flatMap(Int.init) ?? 500
                let year = Self.editions.first { $0.id == id }?.year ?? 2001
                let songs: [[String: Any]] = Self.names.enumerated().map { index, title in
                    ["wrapperType": "track", "kind": "song", "trackName": title, "artistName": "Artist", "discNumber": 1, "trackNumber": index + 1, "trackCount": Self.names.count]
                }
                object = ["results": [album(id, year)] + songs]
            } else { object = ["results": Self.editions.map { album($0.id, $0.year) }] }
        default: code = 404; object = [:]
        }
        let response = HTTPURLResponse(url: request.url!, statusCode: code, httpVersion: "HTTP/1.1", headerFields: nil)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: try! JSONSerialization.data(withJSONObject: object))
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

@main struct MagicLibraryTests {
    static func track(_ path: String, name: String, title: String = "", album: String = "", artist: String = "", albumArtist: String = "", number: Int = 0, folder: String = "") -> MagicLibraryTrack {
        MagicLibraryTrack(id: path, fileName: name, title: title, artist: artist, album: album, albumArtist: albumArtist, disc: 0, track: number, folderName: folder)
    }
    static func service() -> MagicLibraryService {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [MagicFixtureProtocol.self]
        return MagicLibraryService(lookup: AlbumLookupService(session: URLSession(configuration: configuration)))
    }
    static func check(_ condition: @autoclosure () -> Bool, _ message: String) { precondition(condition(), message) }
    static func main() async throws {
        let full = [
            track("/music/Artist - Album [FLAC]/01.flac", name: "01 First Song.flac", title: "Wrong", album: "Wrong album", artist: "Wrong artist", folder: "Artist - Album [FLAC]"),
            track("/music/Artist - Album [FLAC]/02.flac", name: "02 Second Song.flac", title: "Wrong", album: "Wrong album", artist: "Wrong artist", folder: "Artist - Album [FLAC]"),
            track("/music/Artist - Album [FLAC]/03.flac", name: "03 Finale Song.flac", title: "Wrong", album: "Wrong album", artist: "Wrong artist", folder: "Artist - Album [FLAC]")]
        let fullGroups = MagicLibraryService.group(full)
        check(fullGroups.count == 1 && fullGroups[0].tracks.count == 3, "A real album folder stays together when all tags disagree with filenames")
        let outcome = try await service().identify(group: fullGroups[0])
        check(outcome.safeToApply && outcome.trackMatches.count == 3 && outcome.details?.title == "Album", "Exact filenames and folder context safely repair wrong tags")
        check(outcome.details?.cover == nil && !outcome.warnings.isEmpty, "Missing covers or another provider's failure must not discard confident tag fixes")

        let partial = [
            track("/music/Downloads1/a.flac", name: "First Song.flac", album: "Album", artist: "Artist", folder: "Downloads1"),
            track("/music/Downloads2/b.flac", name: "Finale Song.flac", album: "Album", artist: "Artist", folder: "Downloads2")]
        let partialGroups = MagicLibraryService.group(partial)
        check(partialGroups.count == 1, "Reliable logical album tags join selected tracks from generic download folders")
        let partialOutcome = try await service().identify(group: partialGroups[0])
        check(partialOutcome.safeToApply && partialOutcome.trackMatches[partial[1].id]?.track == 3,
              "A partial selection maps uniquely by title, not by its shorter selection order")
        let discs = [track("/music/Artist - Album/CD1/first.flac", name: "01 First Song.flac", folder: "CD1"),
                     track("/music/Artist - Album/CD2/last.flac", name: "01 Finale Song.flac", folder: "CD2")]
        check(MagicLibraryService.group(discs).count == 1, "Disc subfolders belong to their common parent album")
        let quality = [track("/flac/Artist - Album [FLAC]/first.flac", name: "First Song.flac", folder: "Artist - Album [FLAC]"),
                       track("/mp3/Artist - Album [MP3]/last.mp3", name: "Finale Song.mp3", folder: "Artist - Album [MP3]")]
        check(MagicLibraryService.group(quality).count == 1, "Format flags do not split the same selected album")
        let unrelated = quality + [track("/other/Other Artist - Other Album/song.flac", name: "Song.flac", folder: "Other Artist - Other Album")]
        check(MagicLibraryService.group(unrelated).count == 2, "Different catalog titles remain separate")
        let unknown = [track("/music/Music/a.mp3", name: "01.mp3", title: "Unknown", folder: "Music"),
                       track("/music/Music/b.mp3", name: "02.mp3", title: "Unknown", folder: "Music")]
        check(MagicLibraryService.group(unknown).count == 2, "A generic folder cannot merge unrelated unknown tracks")
        let unknownOutcome = try await service().identify(group: MagicLibraryService.group(unknown)[0])
        check(!unknownOutcome.safeToApply && unknownOutcome.trackMatches.isEmpty, "Numbers and a generic Music folder never authorize an automatic replacement")

        let source = ["First Song", "Second Song", "Third Song", "Fourth Song"].enumerated().map {
            AlbumLookupTrack(title: $0.element, artist: "Artist", disc: 1, track: $0.offset + 1, trackTotal: 4, duration: nil)
        }
        let numbered = [track("/album/a", name: "01 First Song.flac"), track("/album/b", name: "02 Second Song.flac"),
                        track("/album/c", name: "03.flac"), track("/album/d", name: "04.flac")]
        let completed = MagicLibraryService.match(numbered, to: source)
        check(completed.count == 4 && completed[numbered[3].id]?.title == "Fourth Song", "Confirmed titles plus complete filename order can repair remaining missing names")
        let inconsistent = [track("/album/a", name: "02 First Song.flac"), track("/album/b", name: "01 Second Song.flac"),
                            track("/album/c", name: "03.flac"), track("/album/d", name: "04.flac")]
        check(MagicLibraryService.match(inconsistent, to: source).count == 2, "A confirmed title that contradicts numbering prevents ordered completion")
        let allNumbers = (1...4).map { track("/album/\($0)", name: String(format: "%02d.flac", $0)) }
        check(MagicLibraryService.match(allNumbers, to: source).isEmpty, "A full count with numeric-only names is still insufficient evidence")
        let discSource = source.enumerated().map { index, song in
            AlbumLookupTrack(title: song.title, artist: song.artist, disc: index / 2 + 1, track: index % 2 + 1, trackTotal: 2, duration: nil)
        }
        let numberedDiscs = [track("/album/CD1/a", name: "01 First Song.flac"), track("/album/CD1/b", name: "02 Second Song.flac"),
                             track("/album/CD2/c", name: "01.flac"), track("/album/CD2/d", name: "02.flac")]
        check(MagicLibraryService.match(numberedDiscs, to: discSource).count == 4, "CD folder numbering supplies an unambiguous disc when completing a confirmed full album")
        let duplicateSource = [AlbumLookupTrack(title: "Intro", artist: "A", disc: 1, track: 1, trackTotal: 2, duration: nil),
                               AlbumLookupTrack(title: "Intro", artist: "A", disc: 1, track: 2, trackTotal: 2, duration: nil)]
        check(MagicLibraryService.match([track("/intro", name: "Intro.mp3")], to: duplicateSource).isEmpty, "Repeated source titles are ambiguous for a partial selection")
        let comparisonTracks = [partial[0], track(partial[1].id, name: "Second Song.flac", album: "Album", artist: "Artist", folder: "Downloads2")]
        let partialGroup = MagicLibraryService.group(comparisonTracks)[0]
        let mapping = MagicLibraryService.match(comparisonTracks, to: source)
        let chosen = AlbumLookupRelease(id: "itunes:10", title: "Album", artist: "Artist", year: 2001, country: "US", trackCount: 4,
                                        sourceURL: URL(string: "https://itunes.apple.com/album/id10")!, provider: .itunes, matchScore: 98)
        var other = AlbumLookupRelease(id: "musicBrainz:20", title: "Album", artist: "Artist", year: 2001, country: "US", trackCount: 4,
                                       sourceURL: URL(string: "https://musicbrainz.org/release/example")!, matchScore: 97)
        let sourceDetails = AlbumLookupDetails(title: "Album", artist: "Artist", year: 2001, genre: nil, tracks: source, cover: nil, coverWarning: nil)
        let identical = MagicLibraryService.compareEditions(group: partialGroup, chosen: chosen, details: sourceDetails, matches: mapping,
                                                            alternatives: [(other, sourceDetails, mapping)])
        check(!identical.requiresReview && !identical.preserveYear && !identical.preserveCover, "The same album represented by two providers is not an ambiguous edition")
        let shifted = source.map { AlbumLookupTrack(title: $0.title, artist: $0.artist, disc: $0.disc, track: $0.track + 1, trackTotal: $0.trackTotal + 1, duration: nil) }
        let shiftedDetails = AlbumLookupDetails(title: "Album", artist: "Artist", year: 2001, genre: nil, tracks: shifted, cover: nil, coverWarning: nil)
        let numbering = MagicLibraryService.compareEditions(group: partialGroup, chosen: chosen, details: sourceDetails, matches: mapping,
                                                            alternatives: [(other, shiftedDetails, MagicLibraryService.match(comparisonTracks, to: shifted))])
        check(numbering.requiresReview && numbering.preserveCover, "Near-tied editions with different selected track positions require review")
        let otherAlbum = AlbumLookupDetails(title: "Different Album", artist: "Artist", year: 2001, genre: nil, tracks: source, cover: nil, coverWarning: nil)
        check(MagicLibraryService.compareEditions(group: partialGroup, chosen: chosen, details: sourceDetails, matches: mapping,
                                                  alternatives: [(other, otherAlbum, mapping)]).requiresReview, "Near-tied distinct album identities must not be guessed")
        let reissue = AlbumLookupDetails(title: "Album", artist: "Artist", year: 2015, genre: nil, tracks: source, cover: nil, coverWarning: nil)
        let dates = MagicLibraryService.compareEditions(group: partialGroup, chosen: chosen, details: sourceDetails, matches: mapping,
                                                       alternatives: [(other, reissue, mapping)])
        check(!dates.requiresReview && dates.preserveYear && dates.preserveCover, "Year-only uncertainty preserves year and edition artwork while allowing common tag fixes")
        other.matchScore = 80
        check(!MagicLibraryService.compareEditions(group: partialGroup, chosen: chosen, details: sourceDetails, matches: mapping,
                                                   alternatives: [(other, shiftedDetails, MagicLibraryService.match(comparisonTracks, to: shifted))]).requiresReview,
              "A clearly weaker edition does not block the best supported result")
        let fullNamed = source.enumerated().map { index, song in track("/numbered/\(index)", name: String(format: "%02d %@.flac", index + 1, song.title), album: "Album", artist: "Artist", folder: "Artist - Album") }
        let fullNamedGroup = MagicLibraryService.group(fullNamed)[0]
        let bonusTracks = source.map { AlbumLookupTrack(title: $0.title, artist: $0.artist, disc: $0.disc, track: $0.track, trackTotal: 5, duration: nil) } + [AlbumLookupTrack(title: "Bonus Song", artist: "Artist", disc: 1, track: 5, trackTotal: 5, duration: nil)]
        let bonusDetails = AlbumLookupDetails(title: "Album", artist: "Artist", year: 2001, genre: nil, tracks: bonusTracks, cover: nil, coverWarning: nil)
        other.matchScore = 97
        check(!MagicLibraryService.compareEditions(group: fullNamedGroup, chosen: chosen, details: sourceDetails, matches: MagicLibraryService.match(fullNamed, to: source),
                                                   alternatives: [(other, bonusDetails, MagicLibraryService.match(fullNamed, to: bonusTracks))]).requiresReview,
              "A complete numbered rip identifies its count, so an extra-bonus-track edition does not block common fixes")
        MagicFixtureProtocol.editions = [(500, 2001), (501, 2015)]
        MagicFixtureProtocol.includeCover = true
        MagicFixtureProtocol.coverRequests = 0
        let yearOutcome = try await service().identify(group: fullGroups[0])
        check(yearOutcome.safeToApply && yearOutcome.details?.year == nil && yearOutcome.details?.cover == nil,
              "Magic still fixes common metadata when reissue years are uncertain, without replacing year or cover")
        check(MagicFixtureProtocol.coverRequests == 0 && yearOutcome.warnings.contains { $0.contains("different release years") },
              "Uncertain edition artwork is not downloaded or silently substituted")
        let datedGroup = MagicLibraryGroup(id: "dated", tracks: full, query: AlbumLookupQuery(artist: "Artist", album: "Album", folderName: "Artist - Album (2015)", tracks: fullGroups[0].query.tracks), summary: "Dated album")
        let dated = try await service().identify(group: datedGroup)
        check(dated.safeToApply && dated.details?.year == 2015 && dated.release?.id == "itunes:501" && dated.details?.cover == MagicFixtureProtocol.png,
              "An explicit folder edition year can select the matching release and its cover")
        let beforeMapped = Set(try FileManager.default.contentsOfDirectory(atPath: FileManager.default.temporaryDirectory.path).filter { $0.hasPrefix("betterpod-magic-art-") })
        let bytes = Data((0..<(2 * 1024 * 1024)).map { UInt8($0 % 251) })
        let mapped = try MagicLibraryService.mappedArtwork(bytes)
        let afterMapped = Set(try FileManager.default.contentsOfDirectory(atPath: FileManager.default.temporaryDirectory.path).filter { $0.hasPrefix("betterpod-magic-art-") })
        check(beforeMapped == afterMapped, "The mapped-artwork temporary file and directory must be unlinked")
        check(mapped == bytes, "Mapped artwork remains byte-identical and readable after unlinking its temporary file")
        print("Magic grouping, matching, edition ambiguity and mapped artwork checks passed")
    }
}
