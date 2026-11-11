// Run with:
// swiftc mac/App/AlbumLookup.swift mac/App/AlbumLookupDiscogs.swift mac/App/AlbumLookupWikipedia.swift mac/tests/AlbumLookupTests.swift -o /tmp/bp-lookup-tests && /tmp/bp-lookup-tests

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

private let englishWikitext = """
{{Short description|1997 album}}
{{Infobox album
| name       = Second Light<!-- comment | with pipe -->
| type       = studio
| artist     = [[The Lanterns]]
| cover      = File:Second_Light_cover.png
| released   = {{Start date|1997|5|21|df=y}}<ref>{{cite web|title=Release|url=https://example.com}}</ref>
| genre      = {{hlist|[[alternative rock]]|[[art rock]]}}
| length     = 12:00
}}
'''''Second Light''''' is an album.

== Track listing ==
{{Track listing
| headline = Disc one
| title1 = Opening
| length1 = 3:30
| title2 = [[Paper Song|Paper]]
| length2 = 4:05
}}
{{Track listing
| headline = Disc two
| extra_column = Performer
| title1 = Finale
| extra1 = [[Guest Singer]]
| length1 = 1:02:03
}}
{{Track listing
| headline = Japanese edition bonus track
| title3 = Extra
}}
"""

private let italianWikitext = """
{{Album
|titolo = Canzoni di mare
|artista = [[Gruppo Esempio]]
|anno = 1983
|genere = Pop rock
|immagine = Copertina.jpg
}}
== Tracce ==
; Lato A
# ''Onda'' – 3:10
# ''Scogli'' – 4:00
; Lato B
# "Faro" – 2:50
== Formazione ==
# Not a track
"""

private func wikipediaPages(_ pages: [[String: Any]]) -> Data {
    try! JSONSerialization.data(withJSONObject: ["batchcomplete": true, "query": ["pages": pages]])
}

private func wikipediaPage(_ id: Int, _ title: String, _ text: String, image: String?, index: Int, host: String = "en") -> [String: Any] {
    var page: [String: Any] = ["pageid": id, "ns": 0, "title": title, "index": index,
                               "fullurl": "https://\(host).wikipedia.org/wiki/\(title.replacingOccurrences(of: " ", with: "_"))",
                               "revisions": [["slots": ["main": ["contentmodel": "wikitext", "content": text]]]]]
    if let image {
        page["pageimage"] = image
        page["thumbnail"] = ["source": "https://upload.wikimedia.org/wikipedia/\(host)/1/12/\(image)", "width": 300, "height": 300]
    }
    return page
}

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
        // Wikipedia: album infoboxes and track listings in English and Italian articles.
        let wikiSearch = wikipediaPages([
            wikipediaPage(41, "Second Light (album)", englishWikitext, image: "Second_Light_cover.png", index: 1),
            wikipediaPage(42, "The Lanterns", "{{Infobox musical artist|name=The Lanterns}}", image: "Band.jpg", index: 2)
        ])
        let englishAlbums = try WikipediaAlbums.parse(wikiSearch, edition: .english)
        require(englishAlbums.count == 1, "Articles without an album infobox must be left out")
        let english = englishAlbums[0]
        require(english.release.title == "Second Light" && english.release.artist == "The Lanterns" && english.release.year == 1997,
                "Infobox name, linked artist and start date year")
        require(english.details.genre == "Alternative rock", "The first genre of a list, capitalized")
        require(english.details.tracks.map(\.title) == ["Opening", "Paper", "Finale"] && english.details.tracks.map(\.disc) == [1, 1, 2],
                "Track listing templates keep link labels, discs follow headlines, bonus editions are skipped")
        require(english.details.tracks.map(\.trackTotal) == [2, 2, 1] && english.details.tracks[2].artist == "Guest Singer" && english.details.tracks[0].artist == "The Lanterns",
                "Per-disc totals; a performer column overrides the album artist")
        require(english.details.tracks[1].duration == 245 && english.details.tracks[2].duration == 3723, "Track lengths in seconds")
        require(english.release.trackCount == 3 && english.release.sourceName == "Wikipedia" && english.release.id == "wikipedia:en:41", "Release identity and count")
        require(english.coverURL?.host == "upload.wikimedia.org", "The infobox cover file is the page image")
        let italian = try WikipediaAlbums.parse(wikipediaPages([wikipediaPage(7, "Canzoni di mare", italianWikitext, image: "Altro.jpg", index: 1, host: "it")]), edition: .italian)
        require(italian.count == 1 && italian[0].release.title == "Canzoni di mare" && italian[0].release.artist == "Gruppo Esempio" && italian[0].release.year == 1983,
                "Italian album template fields")
        require(italian[0].details.tracks.map(\.title) == ["Onda", "Scogli", "Faro"] && italian[0].details.tracks.map(\.track) == [1, 2, 3],
                "Numbered lists under Tracce continue across vinyl sides and stop at the next section")
        require(italian[0].coverURL == nil && italian[0].release.sourceName == "Wikipedia (it)", "A page image that is not the infobox cover is not used")
        require(WikipediaAlbums.firstYear("May 21, 1997") == 1997 && WikipediaAlbums.duration("4:44") == 284, "Plain dates and lengths")
        require(WikiText.plain("[[Rock music|Rock]]<br />''Pop''") == "Rock, Pop", "Links, line breaks and italics become plain text")
        do {
            _ = try WikipediaAlbums.parse(Data(#"{"error":{"code":"maxlag","info":"Waiting for a database server"}}"#.utf8), edition: .english)
            fatalError("A Wikipedia API error must be reported")
        } catch AlbumLookupError.service("Wikipedia", let message) where message.contains("database") { }

        FixtureProtocol.requests = []
        FixtureProtocol.reply = { request in
            switch request.url?.host {
            case "en.wikipedia.org": return .init(code: 200, body: wikiSearch)
            case "it.wikipedia.org": return .init(code: 503, body: Data())
            case "upload.wikimedia.org": return .init(code: 200, body: png)
            case "itunes.apple.com": return .init(code: 200, body: Data(#"{"results":[]}"#.utf8))
            case "api.deezer.com": return .init(code: 200, body: Data(#"{"data":[]}"#.utf8))
            case "musicbrainz.org": return .init(code: 200, body: Data(#"{"releases":[]}"#.utf8))
            default: return .init(code: 404, body: Data())
            }
        }
        let wikiService = AlbumLookupService(session: URLSession(configuration: configuration))
        let wikiFound = try await wikiService.search(query: AlbumLookupQuery(artist: "The Lanterns", album: "Second Light", tracks: [
            AlbumLookupLocalTrack(fileName: "1-01 Opening.flac", title: "Opening"), AlbumLookupLocalTrack(fileName: "1-02 Paper.flac", title: "Paper"),
            AlbumLookupLocalTrack(fileName: "2-01 Finale.flac", title: "Finale")]))
        require(wikiFound.releases.first?.provider == .wikipedia && (wikiFound.releases.first?.matchScore ?? 0) > 80,
                "An album found only on Wikipedia is ranked with its track list")
        let wikiSearchRequest = FixtureProtocol.requests.map(\.0).first { $0.url?.host == "en.wikipedia.org" }
        let wikiTerms = wikiSearchRequest.flatMap { URLComponents(url: $0.url!, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "gsrsearch" }?.value } ?? ""
        require(wikiTerms.contains("hastemplate:\"Infobox album\"") && wikiTerms.contains("Second Light"), "Searches are limited to album articles")
        let pageRequests = FixtureProtocol.requests.count
        let wikiDetails = try await wikiService.details(for: wikiFound.releases[0])
        require(wikiDetails.tracks.count == 3 && wikiDetails.cover == png && wikiDetails.coverSource == "Wikipedia",
                "Wikipedia details reuse the searched article and download its cover")
        require(FixtureProtocol.requests.dropFirst(pageRequests).allSatisfy { $0.0.url?.host == "upload.wikimedia.org" },
                "The article is not downloaded a second time")
        // Discogs: CD editions first, one edition per album, positions to discs.
        let discogsSearch = Data(#"""
        {"pagination":{"items":3},"results":[
          {"id":501,"type":"release","master_id":90,"title":"Nirvana (2) - Second Light","year":"1997","country":"UK","format":["CD","Album"],"uri":"/release/501-Second-Light","cover_image":"https://i.discogs.com/abc/cover.jpeg"},
          {"id":502,"type":"release","master_id":90,"title":"Nirvana (2) - Second Light","year":"1998","format":["CD"]},
          {"id":503,"type":"release","master_id":0,"title":"Other - Second Light - Live","year":"","cover_image":"https://st.discogs.com/images/spacer.gif"}
        ]}
        """#.utf8)
        let discogsReleases = try DiscogsAlbums.parseSearch(discogsSearch)
        require(discogsReleases.map(\.id) == ["discogs:release:501", "discogs:release:503"], "One edition per master release")
        require(discogsReleases[0].artist == "Nirvana" && discogsReleases[0].title == "Second Light" && discogsReleases[0].year == 1997,
                "Discogs titles split into artist and album; numbered artist names are cleaned")
        require(discogsReleases[0].coverURL?.host == "i.discogs.com" && discogsReleases[1].coverURL == nil && discogsReleases[1].title == "Second Light - Live",
                "Discogs covers, placeholder images and titles with dashes")
        require(discogsReleases[0].sourceName == "Discogs · CD, Album", "Show the edition's format")
        let discogsRelease = Data(#"""
        {"id":501,"title":"Second Light","year":0,"released":"1997-05-21","genres":["Electronic","Rock"],"styles":["Alternative Rock"],
         "artists":[{"name":"Nirvana (2)","anv":"","join":"&"},{"name":"Guest","anv":"The Guest","join":""}],
         "images":[{"type":"secondary","uri":"https://i.discogs.com/back.jpeg"},{"type":"primary","uri":"https://i.discogs.com/front.jpeg"}],
         "tracklist":[
           {"position":"","type_":"heading","title":"Disc One"},
           {"position":"1-1","type_":"track","title":"Opening","duration":"3:30"},
           {"position":"1-2","type_":"track","title":"Paper","duration":"4:05","artists":[{"name":"Soloist*","join":""}]},
           {"position":"2-1","type_":"track","title":"Finale","duration":""}
         ]}
        """#.utf8)
        let discogsDetails = try DiscogsAlbums.parseDetails(discogsRelease)
        require(discogsDetails.details.artist == "Nirvana & The Guest" && discogsDetails.details.year == 1997 && discogsDetails.details.genre == "Alternative Rock",
                "Artist credits, release date year and a precise style when genres are broad")
        require(discogsDetails.details.tracks.map(\.title) == ["Opening", "Paper", "Finale"] && discogsDetails.details.tracks.map(\.disc) == [1, 1, 2] &&
                discogsDetails.details.tracks.map(\.trackTotal) == [2, 2, 1], "Headings are skipped; disc-track positions keep their discs")
        require(discogsDetails.details.tracks[1].artist == "Soloist" && discogsDetails.details.tracks[0].duration == 210 && discogsDetails.details.tracks[2].duration == nil,
                "Track artists and lengths")
        require(discogsDetails.coverURL?.lastPathComponent == "front.jpeg", "The primary image is the cover")
        let vinyl = try DiscogsAlbums.parseDetails(Data(#"{"title":"LP","genres":["Hip Hop"],"styles":["Jazzy Hip-Hop"],"tracklist":[{"position":"A1","title":"One"},{"position":"A2","title":"Two"},{"position":"B1","title":"Three"}]}"#.utf8))
        require(vinyl.details.tracks.map(\.track) == [1, 2, 3] && vinyl.details.tracks.allSatisfy { $0.disc == 1 } && vinyl.details.genre == "Hip Hop",
                "Vinyl sides count from one on a single disc; a single genre stays broad")
        require(DiscogsAlbums.link(in: "https://www.discogs.com/it/release/2621910-DJ-Gruff-Rapadopa?srsltid=AYC") == "release:2621910" &&
                DiscogsAlbums.link(in: "discogs.com/master/406159-DJ-Gruff-Rapadopa") == "master:406159" &&
                DiscogsAlbums.link(in: "Rapadopa") == nil, "Recognize pasted Discogs release and master links")

        FixtureProtocol.requests = []
        FixtureProtocol.reply = { request in
            let items = URLComponents(url: request.url!, resolvingAgainstBaseURL: false)?.queryItems ?? []
            switch request.url?.host {
            case "api.discogs.com":
                if request.url?.path == "/releases/501" || request.url?.path == "/releases/777" { return .init(code: 200, body: discogsRelease) }
                if items.contains(where: { $0.name == "format" }) { return .init(code: 200, body: Data(#"{"results":[]}"#.utf8)) }
                return .init(code: 200, body: discogsSearch)
            case "i.discogs.com": return .init(code: 200, body: png)
            case "itunes.apple.com": return .init(code: 200, body: Data(#"{"results":[]}"#.utf8))
            case "api.deezer.com": return .init(code: 200, body: Data(#"{"data":[]}"#.utf8))
            case "musicbrainz.org": return .init(code: 200, body: Data(#"{"releases":[]}"#.utf8))
            case "en.wikipedia.org", "it.wikipedia.org": return .init(code: 200, body: Data(#"{"batchcomplete":true}"#.utf8))
            default: return .init(code: 404, body: Data())
            }
        }
        let discogsService = AlbumLookupService(session: URLSession(configuration: configuration))
        let discogsTracks = [AlbumLookupLocalTrack(fileName: "1-01 Opening.flac", title: "Opening"), AlbumLookupLocalTrack(fileName: "1-02 Paper.flac", title: "Paper"),
                             AlbumLookupLocalTrack(fileName: "2-01 Finale.flac", title: "Finale")]
        let discogsFound = try await discogsService.search(query: AlbumLookupQuery(artist: "Nirvana", album: "Second Light", tracks: discogsTracks))
        require(discogsFound.releases.first?.id == "discogs:release:501" && (discogsFound.releases.first?.matchScore ?? 0) > 80, "A Discogs edition is ranked by its track list")
        let discogsSearches = FixtureProtocol.requests.map(\.0).filter { $0.url?.path == "/database/search" }
        require(discogsSearches.first.map { URLComponents(url: $0.url!, resolvingAgainstBaseURL: false)?.queryItems?.contains { $0.name == "format" && $0.value == "CD" } == true } == true &&
                discogsSearches.count >= 2, "Search CD editions first, then every format")
        let discogsCover = try await discogsService.details(for: discogsFound.releases[0])
        require(discogsCover.cover == png && discogsCover.coverSource == "Discogs" && discogsCover.tracks.count == 3, "Discogs details and cover")
        FixtureProtocol.requests = []
        let pasted = try await discogsService.search(query: AlbumLookupQuery(artist: "", album: "https://www.discogs.com/it/release/777-Second-Light", tracks: discogsTracks))
        require(pasted.releases.count == 1 && pasted.releases[0].title == "Second Light" && pasted.releases[0].trackCount == 3 && (pasted.releases[0].matchScore ?? 0) > 80,
                "A pasted Discogs link opens that edition")
        require(FixtureProtocol.requests.allSatisfy { $0.0.url?.path == "/releases/777" }, "A pasted link is not searched for")
        print("Album lookup fixtures, fuzzy inference, provider fallback, Discogs, Wikipedia and networking checks passed")
    }
}
