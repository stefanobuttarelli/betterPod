// Run with:
// swiftc mac/App/AlbumLookup.swift mac/tests/AlbumLookupTests.swift -o /tmp/bp-lookup-tests && /tmp/bp-lookup-tests

import Foundation

private let releaseID = "76df3287-6cda-33eb-8e9a-044b5e15ffdd"
private let searchFixture = Data("""
{"releases":[
  {"id":"76df3287-6cda-33eb-8e9a-044b5e15ffdd","title":"Album","date":"2001-03-04","country":"IT",
   "artist-credit":[{"name":"One","joinphrase":" & "},{"artist":{"name":"Two"}}],
   "media":[{"track-count":2},{"track-count":1}]},
  {"id":"76df3287-6cda-33eb-8e9a-044b5e15ffdd","title":"Duplicate"},
  {"id":"not-an-id","title":"Invalid"}
]}
""".utf8)

private let detailsFixture = Data("""
{"id":"76df3287-6cda-33eb-8e9a-044b5e15ffdd","title":"Album","date":"2001",
 "artist-credit":[{"name":"Album artist"}],
 "genres":[{"name":"rock","count":2},{"name":"jazz","count":9}],
 "media":[
   {"position":2,"track-count":1,"tracks":[
     {"position":1,"number":"B1","title":"Finale","artist-credit":[{"name":"Soloist"}],"length":90000}]},
   {"position":1,"track-count":2,"tracks":[
     {"position":2,"recording":{"title":"Second","length":123000,"artist-credit":[{"name":"Recording artist"}]}},
     {"position":1,"title":"First","length":0}]}
 ]}
""".utf8)

private final class FixtureProtocol: URLProtocol {
    struct Reply {
        let code: Int
        let body: Data
        var length: Int?
    }
    static let lock = NSLock()
    static var reply: (URLRequest) -> Reply = { _ in Reply(code: 500, body: Data()) }
    static var requests: [(URLRequest, Date)] = []

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        Self.lock.lock()
        Self.requests.append((request, Date()))
        let reply = Self.reply(request)
        Self.lock.unlock()
        var headers: [String: String] = [:]
        if let length = reply.length { headers["Content-Length"] = String(length) }
        let response = HTTPURLResponse(url: request.url!, statusCode: reply.code,
                                       httpVersion: "HTTP/1.1", headerFields: headers)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: reply.body)
        client?.urlProtocolDidFinishLoading(self)
    }
    override func stopLoading() {}
}

private func require(_ condition: @autoclosure () -> Bool, _ message: String) {
    if !condition() { fatalError(message) }
}

@main
struct AlbumLookupTests {
    static func main() async throws {
        let query = try AlbumLookupService.searchQuery(artist: " A ", album: #"A "quote" \ path"#)
        require(query == #"release:"A \"quote\" \\ path" AND artist:"A""#, "Search terms must remain quoted literals")
        do {
            _ = try AlbumLookupService.searchQuery(artist: "A", album: "  ")
            fatalError("An empty album query should fail")
        } catch AlbumLookupError.query { }
        let releases = try AlbumLookupService.parseSearch(searchFixture)
        require(releases.count == 1, "Invalid IDs and duplicate releases must be omitted")
        require(releases[0].artist == "One & Two", "Artist credits must keep join phrases")
        require(releases[0].trackCount == 3 && releases[0].year == 2001, "Multi-disc search count and year")
        let details = try AlbumLookupService.parseDetails(detailsFixture)
        require(details.tracks.map(\.title) == ["First", "Second", "Finale"], "Tracks must sort by disc and position")
        require(details.tracks[0].artist == "Album artist", "Missing track credits inherit the album artist")
        require(details.tracks[1].artist == "Recording artist", "Recording artist fallback")
        require(details.tracks[2].artist == "Soloist", "Track credits take precedence")
        require(details.tracks[1].duration == 123 && details.tracks[0].duration == nil, "Duration is in seconds")
        require(details.tracks[0].trackTotal == 2 && details.tracks[2].trackTotal == 1, "Track totals belong to each disc")
        require(details.genre == "jazz", "Select the highest voted genre")
        for fixture in [
            #"{"id":"x","title":"Album","media":[{"track-count":2,"tracks":[{"title":"Only one"}]}]}"#,
            #"{"id":"x","title":"Album","media":[{"track-count":1,"tracks":[{"position":0,"title":"Invalid"}]}]}"#,
            #"{"id":"x","title":"Album","media":[{"tracks":[{"position":1,"title":"A"},{"position":1,"title":"B"}]}]}"#,
            #"{"id":"x","title":"Album","media":[]}"#
        ] {
            do {
                _ = try AlbumLookupService.parseDetails(Data(fixture.utf8))
                fatalError("Incomplete or ambiguous track lists must fail")
            } catch AlbumLookupError.incompleteTracks { }
        }

        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [FixtureProtocol.self]
        let service = AlbumLookupService(session: URLSession(configuration: configuration))
        FixtureProtocol.reply = { request in
            if request.url?.host == "coverartarchive.org" { return .init(code: 404, body: Data()) }
            return .init(code: 200, body: request.url?.path.contains(releaseID) == true ? detailsFixture : searchFixture)
        }
        let found = try await service.search(artist: "One & Two", album: "Album")
        let downloaded = try await service.details(for: found[0])
        require(downloaded.tracks.count == 3 && downloaded.cover == nil, "Missing art must not lose metadata")
        require(downloaded.coverWarning != nil, "A missing cover needs a visible reason")
        let requests = FixtureProtocol.requests
        let metadataRequests = requests.filter { $0.0.url?.host == "musicbrainz.org" }
        require(metadataRequests.count == 2, "Expected a search and a release lookup")
        require(metadataRequests[1].0.url?.absoluteString.contains("recordings%2Bartist-credits%2Bgenres") == true,
                "MusicBrainz include separators must be encoded literal plus signs rather than form-decoded spaces")
        require(metadataRequests.allSatisfy { $0.0.value(forHTTPHeaderField: "Accept") == "application/json" }, "Catalog requests accept JSON, not images")
        require(metadataRequests[1].1.timeIntervalSince(metadataRequests[0].1) >= 1, "Respect MusicBrainz request spacing")
        require(metadataRequests.allSatisfy { $0.0.value(forHTTPHeaderField: "User-Agent")?.contains("betterPod-Mac/") == true }, "Identify the application")

        let png = Data(base64Encoded: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")!
        FixtureProtocol.reply = { request in
            .init(code: 200, body: request.url?.host == "coverartarchive.org" ? png : detailsFixture)
        }
        let withCover = try await service.details(for: found[0])
        require(withCover.cover == png && withCover.coverWarning == nil, "Valid artwork should be returned")
        FixtureProtocol.reply = { request in
            .init(code: 200, body: request.url?.host == "coverartarchive.org" ? Data("invalid image".utf8) : detailsFixture)
        }
        let invalidCover = try await service.details(for: found[0])
        require(invalidCover.cover == nil && invalidCover.coverWarning != nil && invalidCover.tracks.count == 3,
                "Invalid art must not lose tags or clear the current cover")

        FixtureProtocol.reply = { _ in .init(code: 200, body: Data(), length: (4 << 20) + 1) }
        do {
            _ = try await service.search(artist: "A", album: "Album")
            fatalError("Oversized responses must fail before parsing")
        } catch AlbumLookupError.tooLarge { }
        FixtureProtocol.reply = { _ in .init(code: 503, body: Data()) }
        do {
            _ = try await service.search(artist: "A", album: "Album")
            fatalError("HTTP failures must be reported")
        } catch AlbumLookupError.status(_, 503) { }
        let cancelled = Task { try await service.search(artist: "A", album: "Album") }
        cancelled.cancel()
        do {
            _ = try await cancelled.value
            fatalError("Cancelled requests must stop")
        } catch is CancellationError { }
        // Inference uses folder/file evidence even when every existing tag is wrong.
        let inferred = AlbumLookupQuery(folderName: "One & Two - Album [FLAC] (2001)", tracks: [
            AlbumLookupLocalTrack(fileName: "01 First.flac", title: "Wrong title"),
            AlbumLookupLocalTrack(fileName: "02 Second.flac", title: "Wrong title"),
            AlbumLookupLocalTrack(fileName: "03 Finale.flac", title: "Wrong title")
        ]).inferred
        require(inferred.artist == "One & Two" && inferred.album == "Album", "Infer clean artist and album from a release folder")
        require(AlbumLookupMatching.cleanTrack("1-03. Café.m4a") == "Café", "Strip disc and track filename prefixes")
        require(AlbumLookupMatching.similarity("Café — 夜", "CAFE 夜") == 1, "Match Unicode titles without discarding non-Latin text")
        require(!AlbumLookupError.status("MusicBrainz", 503).localizedDescription.contains("is busy"), "A 503 must be reported precisely, not diagnosed as a busy server")
        let albumResult: (Int, String, String) -> Data = { id, title, artist in
            Data("""
            {"results":[{"wrapperType":"collection","collectionId":\(id),"collectionName":"\(title)","artistName":"\(artist)","trackCount":3,"releaseDate":"2001-01-01","artworkUrl100":"https://is1-ssl.mzstatic.com/image/100x100bb.jpg"}]}
            """.utf8)
        }
        let trackResult: (Int, Bool) -> Data = { id, correct in
            let title = correct ? "Album" : "Wrong album"
            let artist = correct ? "One & Two" : "Wrong artist"
            let names = correct ? ["First", "Second", "Finale"] : ["Unrelated", "Other", "Different"]
            return Data("""
            {"results":[
              {"wrapperType":"collection","collectionId":\(id),"collectionName":"\(title)","artistName":"\(artist)","trackCount":3,"primaryGenreName":"Jazz","artworkUrl100":"https://is1-ssl.mzstatic.com/image/100x100bb.jpg"},
              {"wrapperType":"track","kind":"song","trackName":"\(names[0])","artistName":"Singer 1","discNumber":1,"trackNumber":1,"trackCount":2,"trackTimeMillis":120000},
              {"wrapperType":"track","kind":"song","trackName":"\(names[1])","artistName":"Singer 2","discNumber":1,"trackNumber":2,"trackCount":2,"trackTimeMillis":130000},
              {"wrapperType":"track","kind":"song","trackName":"\(names[2])","artistName":"Singer 3","discNumber":2,"trackNumber":1,"trackCount":1,"trackTimeMillis":140000}]}
            """.utf8)
        }
        FixtureProtocol.reply = { request in
            switch request.url?.host {
            case "musicbrainz.org": return .init(code: 503, body: Data("unavailable".utf8))
            case "api.deezer.com": return .init(code: 200, body: Data(#"{"error":{"message":"Quota exceeded","code":4}}"#.utf8))
            case "is1-ssl.mzstatic.com": return .init(code: 200, body: png)
            case "itunes.apple.com":
                let items = URLComponents(url: request.url!, resolvingAgainstBaseURL: false)?.queryItems ?? []
                if request.url?.path == "/lookup" {
                    let id = items.first { $0.name == "id" }?.value ?? "0"
                    return .init(code: 200, body: trackResult(Int(id)!, id == "200"))
                }
                let term = items.first { $0.name == "term" }?.value ?? ""
                return .init(code: 200, body: term.contains("Wrong") ? albumResult(100, "Wrong album", "Wrong artist") : albumResult(200, "Album", "One & Two"))
            default: return .init(code: 404, body: Data())
            }
        }
        let automatic = try await service.search(query: AlbumLookupQuery(artist: "Wrong artist", album: "Wrong album", folderName: inferred.folderName, tracks: inferred.tracks))
        require(automatic.releases.first?.id == "itunes:200", "Filename/folder agreement must beat consistently wrong existing tags")
        require(automatic.releases.first?.provider == .itunes && (automatic.releases.first?.matchScore ?? 0) > 80, "Rank the checked matching edition first")
        require(automatic.warnings.contains { $0.contains("503") } && automatic.warnings.contains { $0.contains("Quota") }, "Provider outages must not hide another provider's results")
        let automaticDetails = try await service.details(for: automatic.releases[0])
        require(automaticDetails.tracks.map(\.title) == ["First", "Second", "Finale"], "iTunes returns complete ordered multi-disc tracks")
        require(automaticDetails.tracks.map(\.trackTotal) == [2, 2, 1] && automaticDetails.genre == "Jazz", "iTunes per-disc totals and genre")
        require(automaticDetails.cover == png && automaticDetails.coverSource == "iTunes", "The selected provider's artwork is downloaded and identified automatically")
        let deezerRelease = try AlbumLookupService.parseDeezerSearch(Data(#"{"data":[{"id":300,"title":"Album","artist":{"name":"One & Two"},"nb_tracks":3,"cover_xl":"https://cdn-images.dzcdn.net/cover.jpg"}]}"#.utf8))[0]
        FixtureProtocol.reply = { request in
            if request.url?.host == "cdn-images.dzcdn.net" { return .init(code: 200, body: png) }
            if request.url?.path == "/album/300/tracks" {
                return .init(code: 200, body: Data(#"{"data":[{"title":"Finale","disk_number":2,"track_position":1,"artist":{"name":"Soloist"},"duration":90}]}"#.utf8))
            }
            return .init(code: 200, body: Data(#"{"title":"Album","artist":{"name":"One & Two"},"release_date":"2001-02-03","nb_tracks":3,"cover_xl":"https://cdn-images.dzcdn.net/cover.jpg","genres":{"data":[{"name":"Jazz"}]},"tracks":{"data":[{"title":"First","disk_number":1,"track_position":1},{"title":"Second","disk_number":1,"track_position":2}],"next":"https://api.deezer.com/album/300/tracks?index=2"}}"#.utf8))
        }
        let deezerDetails = try await service.details(for: deezerRelease)
        require(deezerDetails.tracks.count == 3 && deezerDetails.tracks.last?.disc == 2 && deezerDetails.tracks.last?.duration == 90, "Deezer paging must preserve disc, position and duration")
        require(deezerDetails.cover == png && deezerDetails.year == 2001, "Deezer cover and release year")
        FixtureProtocol.reply = { request in
            switch request.url?.host {
            case "musicbrainz.org": return .init(code: 200, body: detailsFixture)
            case "coverartarchive.org": return .init(code: 404, body: Data())
            case "itunes.apple.com": return .init(code: 200, body: albumResult(400, "Album", "Album artist"))
            case "is1-ssl.mzstatic.com": return .init(code: 200, body: png)
            default: return .init(code: 404, body: Data())
            }
        }
        let coverService = AlbumLookupService(session: URLSession(configuration: configuration))
        let coverFallback = try await coverService.details(for: releases[0])
        require(coverFallback.cover == png && coverFallback.coverWarning == nil && coverFallback.tracks.count == 3 && coverFallback.coverSource == "iTunes",
                "Missing MusicBrainz cover must fall back to a matching provider without changing the track metadata")
        require(inferred.tracks.map(\.track) == [1, 2, 3] && inferred.tracks.map(\.disc) == [1, 1, 1], "Coherent filename numbering is usable even without tags")
        require(AlbumLookupMatching.cleanFolder("1989 [FLAC]") == "1989", "Preserve a legitimate album title made of a year")
        require(AlbumLookupMatching.cleanFolder("EVANGELION [OST] [FLAC]") == "EVANGELION [OST]", "OST is meaningful album evidence, not a quality flag")
        let fuzzyFolder = "NEON GENESIS EVANGELION [FLAC] (1995) [KICA-286]"
        require(AlbumLookupMatching.cleanFolder(fuzzyFolder) == "NEON GENESIS EVANGELION", "Strip formats, years and catalog IDs without requiring a precise release title")
        require(AlbumLookupMatching.similarity("NEON GENESIS EVANGELION", "Evangelion") > 0.8, "A shorter distinctive official title is a plausible match")
        require(AlbumLookupMatching.similarity("Neon Genesis Evangelion", "NGE") > 0.8, "Recognize title initials")
        let fuzzySearch = Data("""
        {"releases":[{"id":"\(releaseID)","title":"EVANGELION","artist-credit":[{"name":"Shiro Sagisu"}],"media":[{"track-count":2}]}]}
        """.utf8)
        let fuzzyDetails = Data("""
        {"id":"\(releaseID)","title":"EVANGELION","artist-credit":[{"name":"Shiro Sagisu"}],"media":[{"position":1,"track-count":2,"tracks":[{"position":1,"title":"Angel Attack"},{"position":2,"title":"Rei I"}]}]}
        """.utf8)
        FixtureProtocol.reply = { request in
            switch request.url?.host {
            case "musicbrainz.org":
                if request.url?.path.contains(releaseID) == true { return .init(code: 200, body: fuzzyDetails) }
                let query = URLComponents(url: request.url!, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "query" }?.value ?? ""
                return .init(code: 200, body: query.contains("release:(") || query.lowercased() == "release:\"evangelion\"" ? fuzzySearch : Data(#"{"releases":[]}"#.utf8))
            case "itunes.apple.com": return .init(code: 200, body: Data(#"{"results":[]}"#.utf8))
            case "api.deezer.com": return .init(code: 200, body: Data(#"{"data":[]}"#.utf8))
            case "coverartarchive.org": return .init(code: 200, body: png)
            default: return .init(code: 404, body: Data())
            }
        }
        let fuzzyService = AlbumLookupService(session: URLSession(configuration: configuration))
        let fuzzy = try await fuzzyService.search(query: AlbumLookupQuery(folderName: fuzzyFolder, tracks: [
            AlbumLookupLocalTrack(fileName: "01 Angel Attack.flac", title: ""), AlbumLookupLocalTrack(fileName: "02 Rei I.flac", title: "")]))
        require(fuzzy.releases.first?.title == "EVANGELION" && (fuzzy.releases.first?.matchScore ?? 0) > 80,
                "Broad token/short keyword searches must find shorter official titles from a non-exact folder name")
        let requestsAfterFuzzy = FixtureProtocol.requests.map(\.0)
        require(requestsAfterFuzzy.contains { request in
            let query = URLComponents(url: request.url!, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "query" }?.value ?? ""
            return query.contains("release:(neon genesis evangelion)")
        }, "MusicBrainz must receive an unquoted broad query when literal search has no results")
        print("Album lookup fixtures, fuzzy inference, provider fallback and networking checks passed")
    }
}
