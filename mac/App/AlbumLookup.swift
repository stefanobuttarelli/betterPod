// Explicit, multi-provider album identification. Results remain in memory until
// the editor applies them; this service never reads or writes music files.
import Foundation
import ImageIO

enum AlbumLookupProvider: String, CaseIterable, Sendable {
    case musicBrainz, itunes, deezer, discogs, wikipedia
    var name: String {
        switch self {
        case .musicBrainz: return "MusicBrainz"
        case .itunes: return "iTunes"
        case .deezer: return "Deezer"
        case .discogs: return "Discogs"
        case .wikipedia: return "Wikipedia"
        }
    }
}

struct AlbumLookupLocalTrack: Sendable {
    let fileName: String
    let title: String
    var disc: Int = 0
    var track: Int = 0
    var duration: TimeInterval? = nil
}

struct AlbumLookupQuery: Sendable {
    var artist: String = ""
    var album: String = ""
    var folderName: String = ""
    var tracks: [AlbumLookupLocalTrack] = []

    var inferred: AlbumLookupQuery {
        var query = self
        let folder = AlbumLookupMatching.cleanFolder(folderName)
        if AlbumLookupMatching.placeholder(query.album) { query.album = "" }
        if AlbumLookupMatching.placeholder(query.artist) { query.artist = "" }
        let parts = folder.components(separatedBy: " - ")
        if parts.count >= 2 {
            if query.artist.isEmpty { query.artist = parts[0].trimmingCharacters(in: .whitespaces) }
            if query.album.isEmpty { query.album = parts.dropFirst().joined(separator: " - ") }
        } else if query.album.isEmpty && !AlbumLookupMatching.placeholder(folder) {
            query.album = folder
        }
        if query.artist.isEmpty {
            let prefixes = tracks.compactMap { track -> String? in
                let title = AlbumLookupMatching.cleanTrack(track.fileName)
                let pieces = title.components(separatedBy: " - ")
                return pieces.count >= 2 ? pieces[0] : nil
            }
            if let first = prefixes.first, prefixes.count == tracks.count,
               prefixes.allSatisfy({ AlbumLookupMatching.normalized($0) == AlbumLookupMatching.normalized(first) }) {
                query.artist = first
            }
        }
        let filePositions = tracks.map { AlbumLookupMatching.filenamePosition($0.fileName) }
        if filePositions.allSatisfy({ $0 != nil }), Set(filePositions.compactMap { $0.map { "\($0.disc):\($0.track)" } }).count == tracks.count {
            query.tracks = zip(tracks, filePositions).map { track, position in
                var track = track
                track.disc = position!.disc
                track.track = position!.track
                return track
            }
        }
        query.artist = query.artist.trimmingCharacters(in: .whitespacesAndNewlines)
        query.album = query.album.trimmingCharacters(in: .whitespacesAndNewlines)
        return query
    }
    var summary: String {
        let query = inferred
        let name = [query.artist, query.album].filter { !$0.isEmpty }.joined(separator: " — ")
        return (name.isEmpty ? "Track names and numbering" : name) + " · \(tracks.count) tracks"
    }
    var searchTerm: String {
        let query = inferred
        let title = query.album.isEmpty ? query.tracks.first.map { AlbumLookupMatching.localTitle($0, artist: query.artist) } ?? "" : query.album
        return [query.artist, title].filter { !$0.isEmpty }.joined(separator: " ")
    }
}

struct AlbumLookupSearchResult: Sendable {
    let releases: [AlbumLookupRelease]
    let warnings: [String]
}

struct AlbumLookupRelease: Identifiable, Sendable {
    let id: String
    let title: String
    let artist: String
    let year: Int?
    let country: String?
    var trackCount: Int?
    let sourceURL: URL
    var provider: AlbumLookupProvider = .musicBrainz
    var providerID: String? = nil
    var coverURL: URL? = nil
    var matchScore: Int? = nil
    var matchSummary: String? = nil
    /// A finer source name than the provider's, such as a Wikipedia language.
    var sourceLabel: String? = nil
    var sourceName: String { sourceLabel ?? provider.name }
}

struct AlbumLookupDetails: Sendable {
    let title: String
    let artist: String
    let year: Int?
    let genre: String?
    let tracks: [AlbumLookupTrack]
    let cover: Data?
    let coverWarning: String?
    var coverSource: String? = nil
}
struct AlbumLookupTrack: Sendable {
    let title: String
    let artist: String
    let disc: Int
    let track: Int
    let trackTotal: Int
    let duration: TimeInterval?
}

enum AlbumLookupError: LocalizedError {
    case query(String), invalidResponse(String), status(String, Int), tooLarge(String), incompleteTracks
    case service(String, String)
    var errorDescription: String? {
        switch self {
        case .query(let message): return message
        case .invalidResponse(let service): return "\(service) returned an unreadable response."
        case .status(let service, let code):
            if code == 429 { return "\(service) limited this request (HTTP 429). Other services can still be used." }
            if code == 503 { return "\(service) is unavailable or rate-limiting requests (HTTP 503). Other services can still be used." }
            if code == 403 { return "\(service) refused access (HTTP 403). Other services can still be used." }
            if code == 404 { return "This release is no longer available on \(service) (HTTP 404)." }
            return "\(service) could not complete the request (HTTP \(code))."
        case .tooLarge(let service): return "The response from \(service) is too large."
        case .incompleteTracks: return "This edition has an incomplete track list. Choose another edition."
        case .service(let service, let message): return "\(service): \(message)"
        }
    }
}

private actor MusicBrainzRequestGate {
    static let shared = MusicBrainzRequestGate()
    private let clock = ContinuousClock()
    private var nextRequest = ContinuousClock.now
    func wait() async throws {
        try Task.checkCancellation()
        let scheduled = max(clock.now, nextRequest)
        nextRequest = scheduled.advanced(by: .milliseconds(1100))
        try await clock.sleep(until: scheduled)
        try Task.checkCancellation()
    }
}

// Apple documents approximately 20 catalog calls per minute. Leave a small
// margin, fail this provider promptly, and keep the other services available.
private actor ItunesRequestBudget {
    static let shared = ItunesRequestBudget()
    private var requests: [ContinuousClock.Instant] = []
    func reserve() throws {
        let now = ContinuousClock.now
        requests.removeAll { $0.duration(to: now) >= .seconds(60) }
        guard requests.count < 18 else {
            throw AlbumLookupError.service("iTunes", "The local search limit was reached. Wait a minute or use another service.")
        }
        requests.append(now)
    }
}

private struct LookupPacket: Sendable {
    let details: AlbumLookupDetails
    let coverURL: URL?
}
private struct ProviderSearch: Sendable {
    let releases: [AlbumLookupRelease]
    let warning: String?
}

actor AlbumLookupService {
    private let session: URLSession
    private let userAgent: String
    private var metadataCache: [String: LookupPacket] = [:]
    // Wikipedia searches already return each article; keep them for details(for:).
    private var wikipediaCache: [String: WikipediaAlbum] = [:]
    init(session: URLSession? = nil) {
        if let session { self.session = session } else {
            let configuration = URLSessionConfiguration.ephemeral
            configuration.timeoutIntervalForRequest = 12
            configuration.timeoutIntervalForResource = 20
            configuration.httpMaximumConnectionsPerHost = 2
            configuration.httpCookieStorage = nil
            configuration.urlCache = nil
            self.session = URLSession(configuration: configuration)
        }
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "local"
        userAgent = "betterPod-Mac/\(version) (https://github.com/stefanobuttarelli/betterPod)"
    }

    // Kept for callers that deliberately request only MusicBrainz.
    func search(artist: String, album: String) async throws -> [AlbumLookupRelease] {
        let query = try Self.searchQuery(artist: artist, album: album)
        let url = try Self.url(host: "musicbrainz.org", path: "/ws/2/release/", items: [
            URLQueryItem(name: "query", value: query), URLQueryItem(name: "fmt", value: "json"), URLQueryItem(name: "limit", value: "25")])
        return try Self.parseSearch(await fetch(url, service: "MusicBrainz", limit: 4 << 20, musicBrainz: true))
    }

    func search(query input: AlbumLookupQuery) async throws -> AlbumLookupSearchResult {
        if let link = DiscogsAlbums.link(in: input.album + " " + input.artist) {
            return try await linkedDiscogsRelease(link, query: input)
        }
        let query = input.inferred
        guard !query.searchTerm.isEmpty else { throw AlbumLookupError.query("Enter an album or artist, or open tracks with recognizable names.") }
        guard query.artist.count <= 256, query.album.count <= 256, query.folderName.count <= 1024 else {
            throw AlbumLookupError.query("Shorten the album or artist search to 256 characters.")
        }
        try Task.checkCancellation()
        metadataCache = [:]
        wikipediaCache = [:]
        var releases: [AlbumLookupRelease] = []
        var warnings: [String] = []
        await withTaskGroup(of: ProviderSearch.self) { group in
            for provider in AlbumLookupProvider.allCases {
                group.addTask { await self.searchSafely(provider, query: query) }
            }
            for await result in group {
                releases += result.releases
                if let warning = result.warning { warnings.append(warning) }
            }
        }
        try Task.checkCancellation()
        var seen = Set<String>()
        releases = releases.filter { seen.insert($0.id).inserted }.map { AlbumLookupMatching.rank($0, query: query) }
        releases.sort(by: AlbumLookupMatching.precedes)
        // Track lists improve identification without downloading several covers.
        // A selected edition's full-sized image is fetched only in details(for:).
        var candidates: [AlbumLookupRelease] = []
        for provider in AlbumLookupProvider.allCases {
            if let best = releases.first(where: { $0.provider == provider }) { candidates.append(best) }
        }
        var signatures = Set(candidates.map { AlbumLookupMatching.normalized($0.title) + "|" + AlbumLookupMatching.normalized($0.artist) + "|" + String($0.trackCount ?? 0) })
        for release in releases where candidates.count < 5 {
            let signature = AlbumLookupMatching.normalized(release.title) + "|" + AlbumLookupMatching.normalized(release.artist) + "|" + String(release.trackCount ?? 0)
            if signatures.insert(signature).inserted && !candidates.contains(where: { $0.id == release.id }) { candidates.append(release) }
        }
        var refined: [String: AlbumLookupRelease] = [:]
        await withTaskGroup(of: (String, AlbumLookupRelease?, String?).self) { group in
            for release in candidates {
                group.addTask {
                    do {
                        let packet = try await self.metadata(for: release)
                        var checked = AlbumLookupMatching.rank(release, query: query, tracks: packet.details.tracks)
                        if checked.trackCount == nil && !packet.details.tracks.isEmpty { checked.trackCount = packet.details.tracks.count }
                        return (release.id, checked, nil)
                    } catch is CancellationError { return (release.id, nil, nil) }
                    catch {
                        var unchecked = release
                        unchecked.matchScore = max(0, (release.matchScore ?? 0) - 35)
                        unchecked.matchSummary = "Track list unavailable"
                        return (release.id, unchecked, "\(release.provider.name): one edition could not be checked. \(error.localizedDescription)")
                    }
                }
            }
            for await (id, release, warning) in group {
                if let release { refined[id] = release }
                if let warning { warnings.append(warning) }
            }
        }
        try Task.checkCancellation()
        releases = releases.map { refined[$0.id] ?? $0 }
        releases.sort(by: AlbumLookupMatching.precedes)
        if releases.isEmpty && warnings.count == AlbumLookupProvider.allCases.count {
            throw AlbumLookupError.query("No metadata service could complete the search. " + warnings.joined(separator: " "))
        }
        return AlbumLookupSearchResult(releases: Array(releases.prefix(30)), warnings: Array(Set(warnings)).sorted())
    }

    /// A pasted Discogs link names the edition directly; it is compared with
    /// the selected tracks but not searched for.
    private func linkedDiscogsRelease(_ link: String, query: AlbumLookupQuery) async throws -> AlbumLookupSearchResult {
        metadataCache = [:]
        wikipediaCache = [:]
        let source = URL(string: "https://www.discogs.com/" + link.replacingOccurrences(of: ":", with: "/"))!
        let stub = AlbumLookupRelease(id: "discogs:" + link, title: "", artist: "", year: nil, country: nil, trackCount: nil,
                                      sourceURL: source, provider: .discogs, providerID: link)
        let packet = try await metadata(for: stub)
        let release = AlbumLookupRelease(id: stub.id, title: packet.details.title, artist: packet.details.artist, year: packet.details.year,
                                         country: nil, trackCount: packet.details.tracks.count, sourceURL: source, provider: .discogs,
                                         providerID: link, coverURL: packet.coverURL, sourceLabel: "Discogs link")
        let local = AlbumLookupQuery(artist: release.artist, album: release.title, folderName: query.folderName, tracks: query.tracks)
        return AlbumLookupSearchResult(releases: [AlbumLookupMatching.rank(release, query: local, tracks: packet.details.tracks)], warnings: [])
    }

    private func searchSafely(_ provider: AlbumLookupProvider, query: AlbumLookupQuery) async -> ProviderSearch {
        var rows: [AlbumLookupRelease] = []
        var variants = [query]
        let folder = AlbumLookupQuery(folderName: query.folderName, tracks: query.tracks).inferred
        if !folder.album.isEmpty && AlbumLookupMatching.normalized(folder.album) != AlbumLookupMatching.normalized(query.album) {
            variants.append(folder)
        }
        let fileQuery = AlbumLookupQuery(album: folder.album.isEmpty ? query.album : folder.album, tracks: query.tracks).inferred
        if !fileQuery.artist.isEmpty && AlbumLookupMatching.normalized(fileQuery.artist) != AlbumLookupMatching.normalized(query.artist) && fileQuery.artist != folder.artist {
            variants.append(fileQuery)
        }
        let broad = folder.album.isEmpty ? query : folder
        if variants.count < 3, let keyword = AlbumLookupMatching.distinctiveKeyword(broad.album),
           AlbumLookupMatching.normalized(keyword) != AlbumLookupMatching.normalized(broad.album) {
            variants.append(AlbumLookupQuery(artist: broad.artist, album: keyword, tracks: query.tracks))
        }
        do {
            // Discogs allows few requests a minute: the tags and the folder are enough.
            for variant in provider == .discogs ? Array(variants.prefix(2)) : variants {
                rows += try await providerSearch(provider, query: variant)
            }
            return ProviderSearch(releases: rows, warning: nil)
        } catch is CancellationError { return ProviderSearch(releases: [], warning: nil) }
        catch { return ProviderSearch(releases: rows, warning: "\(provider.name): \(error.localizedDescription)") }
    }

    private func providerSearch(_ provider: AlbumLookupProvider, query: AlbumLookupQuery) async throws -> [AlbumLookupRelease] {
        switch provider {
        case .musicBrainz:
            guard !query.album.isEmpty else { return [] }
            var found = try await search(artist: query.artist, album: query.album)
            if found.isEmpty {
                let loose = try Self.broadSearchQuery(artist: query.artist, album: query.album)
                let url = try Self.url(host: "musicbrainz.org", path: "/ws/2/release/", items: [
                    URLQueryItem(name: "query", value: loose), URLQueryItem(name: "fmt", value: "json"),
                    URLQueryItem(name: "limit", value: "12"), URLQueryItem(name: "dismax", value: "true")])
                found = try Self.parseSearch(await fetch(url, service: "MusicBrainz", limit: 4 << 20, musicBrainz: true))
            }
            return found.map { row in
                AlbumLookupRelease(id: "musicBrainz:" + row.id, title: row.title, artist: row.artist,
                                   year: row.year, country: row.country, trackCount: row.trackCount,
                                   sourceURL: row.sourceURL, provider: .musicBrainz, providerID: row.id)
            }
        case .itunes:
            let entity = query.album.isEmpty ? "song" : "album"
            let url = try Self.url(host: "itunes.apple.com", path: "/search", items: [
                URLQueryItem(name: "term", value: query.searchTerm), URLQueryItem(name: "entity", value: entity),
                URLQueryItem(name: "media", value: "music"), URLQueryItem(name: "limit", value: "12"), URLQueryItem(name: "country", value: "US")])
            return try Self.parseItunesSearch(await fetch(url, service: "iTunes", limit: 4 << 20))
        case .deezer:
            let path = query.album.isEmpty ? "/search" : "/search/album"
            let url = try Self.url(host: "api.deezer.com", path: path, items: [URLQueryItem(name: "q", value: query.searchTerm), URLQueryItem(name: "limit", value: "12")])
            return try Self.parseDeezerSearch(await fetch(url, service: "Deezer", limit: 4 << 20))
        case .discogs:
            guard !query.album.isEmpty else { return [] }
            for compactDisc in [true, false] {
                let url = try Self.url(host: "api.discogs.com", path: "/database/search",
                                       items: DiscogsAlbums.searchItems(album: query.album, artist: query.artist, compactDiscOnly: compactDisc))
                let found = try DiscogsAlbums.parseSearch(await fetch(url, service: "Discogs", limit: 4 << 20))
                if !found.isEmpty { return found }
            }
            return []
        case .wikipedia:
            guard !query.album.isEmpty else { return [] }
            var rows: [AlbumLookupRelease] = []
            var failure: Error?
            for edition in WikipediaEdition.all {
                do {
                    let url = try Self.url(host: "\(edition.code).wikipedia.org", path: "/w/api.php",
                                           items: WikipediaAlbums.searchItems(album: query.album, artist: query.artist, edition: edition))
                    let albums = try WikipediaAlbums.parse(await fetch(url, service: "Wikipedia", limit: 8 << 20), edition: edition)
                    for album in albums {
                        if wikipediaCache.count >= 60 { wikipediaCache = [:] }
                        wikipediaCache[album.release.id] = album
                    }
                    rows += albums.map(\.release)
                } catch is CancellationError { throw CancellationError() }
                catch { failure = failure ?? error }
            }
            if rows.isEmpty, let failure { throw failure }
            return rows
        }
    }

    func metadataDetails(for release: AlbumLookupRelease) async throws -> AlbumLookupDetails {
        try await metadata(for: release).details
    }

    func details(for release: AlbumLookupRelease) async throws -> AlbumLookupDetails {
        let packet = try await metadata(for: release)
        var cover: Data?
        var coverSource: String?
        var warnings: [String] = []
        if let url = packet.coverURL ?? release.coverURL {
            do {
                let name = release.provider == .musicBrainz ? "Cover Art Archive" : release.provider.name
                cover = try await artwork(url, service: name)
                coverSource = name
            }
            catch is CancellationError { throw CancellationError() }
            catch let error as URLError where error.code == .cancelled { throw CancellationError() }
            catch { warnings.append(error.localizedDescription) }
        }
        if cover == nil {
            do {
                if let fallback = try await fallbackCover(for: packet.details, excluding: release.provider) {
                    cover = fallback.data
                    coverSource = fallback.source
                }
            }
            catch is CancellationError { throw CancellationError() }
            catch { warnings.append(error.localizedDescription) }
        }
        try Task.checkCancellation()
        let message = cover == nil ? "No cover could be downloaded. Your current cover will be kept." + (warnings.isEmpty ? "" : " " + warnings.joined(separator: " ")) : nil
        return AlbumLookupDetails(title: packet.details.title, artist: packet.details.artist, year: packet.details.year,
                                  genre: packet.details.genre, tracks: packet.details.tracks, cover: cover, coverWarning: message, coverSource: coverSource)
    }

    private func metadata(for release: AlbumLookupRelease) async throws -> LookupPacket {
        if let cached = metadataCache[release.id] { return cached }
        let id = release.providerID ?? release.id.split(separator: ":", maxSplits: 1).last.map(String.init) ?? release.id
        let packet: LookupPacket
        switch release.provider {
        case .musicBrainz:
            guard UUID(uuidString: id) != nil else { throw AlbumLookupError.invalidResponse("MusicBrainz") }
            let url = try Self.url(host: "musicbrainz.org", path: "/ws/2/release/\(id)", items: [
                URLQueryItem(name: "inc", value: "recordings+artist-credits+genres"), URLQueryItem(name: "fmt", value: "json")])
            let details = try Self.parseDetails(await fetch(url, service: "MusicBrainz", limit: 4 << 20, musicBrainz: true))
            packet = LookupPacket(details: details, coverURL: URL(string: "https://coverartarchive.org/release/\(id)/front-1200"))
        case .itunes:
            guard Int64(id) != nil else { throw AlbumLookupError.invalidResponse("iTunes") }
            let url = try Self.url(host: "itunes.apple.com", path: "/lookup", items: [URLQueryItem(name: "id", value: id), URLQueryItem(name: "entity", value: "song"), URLQueryItem(name: "limit", value: "200"), URLQueryItem(name: "country", value: "US")])
            packet = try Self.parseItunesDetails(await fetch(url, service: "iTunes", limit: 4 << 20), release: release)
        case .deezer:
            guard Int64(id) != nil else { throw AlbumLookupError.invalidResponse("Deezer") }
            let url = try Self.url(host: "api.deezer.com", path: "/album/\(id)", items: [])
            var object = try Self.object(await fetch(url, service: "Deezer", limit: 4 << 20), service: "Deezer")
            var container = object["tracks"] as? [String: Any] ?? [:]
            var tracks = container["data"] as? [[String: Any]] ?? []
            var visited = Set<String>()
            for _ in 0..<8 {
                guard let next = container["next"] as? String, !next.isEmpty, visited.insert(next).inserted,
                      let pageURL = URL(string: next), pageURL.scheme == "https", pageURL.host == "api.deezer.com" else { break }
                container = try Self.object(await fetch(pageURL, service: "Deezer", limit: 4 << 20), service: "Deezer")
                tracks += container["data"] as? [[String: Any]] ?? []
                guard tracks.count <= 2000 else { throw AlbumLookupError.tooLarge("Deezer track list") }
            }
            object["tracks"] = ["data": tracks]
            packet = try Self.parseDeezerDetails(object, release: release)
        case .discogs:
            guard let path = DiscogsAlbums.detailsPath(id) else { throw AlbumLookupError.invalidResponse("Discogs") }
            let url = try Self.url(host: "api.discogs.com", path: path, items: [])
            let parsed = try DiscogsAlbums.parseDetails(await fetch(url, service: "Discogs", limit: 4 << 20))
            guard !parsed.details.tracks.isEmpty else { throw AlbumLookupError.incompleteTracks }
            packet = LookupPacket(details: parsed.details, coverURL: parsed.coverURL ?? release.coverURL)
        case .wikipedia:
            if let album = wikipediaCache[release.id] {
                packet = LookupPacket(details: album.details, coverURL: album.coverURL)
                break
            }
            let parts = id.split(separator: ":")
            guard parts.count == 2, let edition = WikipediaEdition.all.first(where: { $0.code == parts[0] }),
                  let pageID = Int(parts[1]), pageID > 0 else { throw AlbumLookupError.invalidResponse("Wikipedia") }
            let url = try Self.url(host: "\(edition.code).wikipedia.org", path: "/w/api.php", items: WikipediaAlbums.pageItems(pageID: pageID))
            guard let album = try WikipediaAlbums.parse(await fetch(url, service: "Wikipedia", limit: 8 << 20), edition: edition).first else {
                throw AlbumLookupError.status("Wikipedia", 404)
            }
            packet = LookupPacket(details: album.details, coverURL: album.coverURL)
        }
        if metadataCache.count >= 12 { metadataCache = [:] }
        metadataCache[release.id] = packet
        return packet
    }

    private func artwork(_ url: URL, service: String) async throws -> Data {
        guard Self.allowedArtworkURL(url) else { throw AlbumLookupError.invalidResponse(service) }
        let data = try await fetch(url, service: service, limit: 8 << 20, image: true)
        guard Self.validCover(data) else { throw AlbumLookupError.invalidResponse(service) }
        return data
    }

    private func fallbackCover(for details: AlbumLookupDetails, excluding: AlbumLookupProvider) async throws -> (data: Data, source: String)? {
        let query = AlbumLookupQuery(artist: details.artist, album: details.title, tracks: details.tracks.map {
            AlbumLookupLocalTrack(fileName: "", title: $0.title, disc: $0.disc, track: $0.track, duration: $0.duration)
        })
        for provider in [AlbumLookupProvider.itunes, .deezer, .discogs] where provider != excluding {
            try Task.checkCancellation()
            do {
                let rows = try await providerSearch(provider, query: query)
                let matched = rows.filter { AlbumLookupMatching.similarity($0.title, details.title) >= 0.8 &&
                    (details.artist.isEmpty || AlbumLookupMatching.similarity($0.artist, details.artist) >= 0.75) }
                    .map { AlbumLookupMatching.rank($0, query: query) }.sorted(by: AlbumLookupMatching.precedes)
                if let best = matched.first {
                    let url: URL?
                    if let known = best.coverURL { url = known }
                    else { url = try await metadata(for: best).coverURL }
                    if let url { return (try await artwork(url, service: provider.name), provider.name) }
                }
            } catch is CancellationError { throw CancellationError() }
            catch { continue }
        }
        return nil
    }

    private func fetch(_ url: URL, service: String, limit: Int, musicBrainz: Bool = false, image: Bool = false) async throws -> Data {
        if musicBrainz { try await MusicBrainzRequestGate.shared.wait() }
        if url.host == "itunes.apple.com" { try await ItunesRequestBudget.shared.reserve() }
        if url.host == "api.discogs.com" { try await DiscogsRequestBudget.shared.reserve() }
        try Task.checkCancellation()
        var request = URLRequest(url: url)
        request.timeoutInterval = 10
        request.setValue(userAgent, forHTTPHeaderField: "User-Agent")
        request.setValue(image ? "image/jpeg, image/png" : "application/json", forHTTPHeaderField: "Accept")
        let (bytes, response) = try await session.bytes(for: request)
        defer { bytes.task.cancel() }
        guard let response = response as? HTTPURLResponse else { throw AlbumLookupError.invalidResponse(service) }
        guard (200..<300).contains(response.statusCode) else { throw AlbumLookupError.status(service, response.statusCode) }
        guard response.expectedContentLength <= Int64(limit) else { throw AlbumLookupError.tooLarge(service) }
        var data = Data()
        if response.expectedContentLength > 0 { data.reserveCapacity(Int(response.expectedContentLength)) }
        var chunk: [UInt8] = []
        chunk.reserveCapacity(32 * 1024)
        for try await byte in bytes {
            guard data.count + chunk.count < limit else { throw AlbumLookupError.tooLarge(service) }
            chunk.append(byte)
            if chunk.count == 32 * 1024 {
                try Task.checkCancellation()
                data.append(contentsOf: chunk)
                chunk.removeAll(keepingCapacity: true)
            }
        }
        data.append(contentsOf: chunk)
        try Task.checkCancellation()
        return data
    }

    private static func url(host: String, path: String, items: [URLQueryItem]) throws -> URL {
        var components = URLComponents()
        components.scheme = "https"
        components.host = host
        components.path = path
        components.queryItems = items.isEmpty ? nil : items
        // MusicBrainz separates include values with a literal '+'. URLQueryItem
        // otherwise leaves '+' unescaped and form-style parsers read a space.
        components.percentEncodedQuery = components.percentEncodedQuery?.replacingOccurrences(of: "+", with: "%2B")
        guard let url = components.url else { throw AlbumLookupError.query("The search could not be prepared.") }
        return url
    }

    private static func allowedArtworkURL(_ url: URL) -> Bool {
        guard url.scheme == "https", let host = url.host?.lowercased() else { return false }
        return ["coverartarchive.org", "archive.org", "mzstatic.com", "dzcdn.net", "discogs.com", "wikimedia.org"].contains { host == $0 || host.hasSuffix("." + $0) }
    }

    private static func object(_ data: Data, service: String) throws -> [String: Any] {
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw AlbumLookupError.invalidResponse(service) }
        if let error = object["error"] as? [String: Any] {
            let message = String((error["message"] as? String ?? "The service rejected the request.").prefix(256))
            throw AlbumLookupError.service(service, message)
        }
        return object
    }
    private static func integer(_ value: Any?) -> Int? {
        if let value = value as? Int { return value }
        if let value = value as? String { return Int(value) }
        return nil
    }
    private static func imageURL(_ text: String?) -> URL? {
        guard let text else { return nil }
        let large = text.replacingOccurrences(of: "100x100bb", with: "1200x1200bb")
        guard var components = URLComponents(string: large) else { return nil }
        if components.scheme == "http" { components.scheme = "https" }
        guard let url = components.url, allowedArtworkURL(url) else { return nil }
        return url
    }
    nonisolated static func parseItunesSearch(_ data: Data) throws -> [AlbumLookupRelease] {
        let object = try object(data, service: "iTunes")
        guard let rows = object["results"] as? [[String: Any]] else { throw AlbumLookupError.invalidResponse("iTunes") }
        var seen = Set<Int>()
        return rows.compactMap { row in
            guard let id = integer(row["collectionId"]), id > 0, seen.insert(id).inserted,
                  let title = row["collectionName"] as? String, !title.isEmpty else { return nil }
            let source = (row["collectionViewUrl"] as? String).flatMap(URL.init(string:)) ?? URL(string: "https://itunes.apple.com/album/id\(id)")!
            return AlbumLookupRelease(id: "itunes:\(id)", title: title, artist: row["artistName"] as? String ?? "",
                                      year: year(row["releaseDate"] as? String), country: row["country"] as? String,
                                      trackCount: integer(row["trackCount"]), sourceURL: source, provider: .itunes,
                                      providerID: String(id), coverURL: imageURL(row["artworkUrl100"] as? String))
        }
    }
    private static func parseItunesDetails(_ data: Data, release: AlbumLookupRelease) throws -> LookupPacket {
        let object = try object(data, service: "iTunes")
        guard let rows = object["results"] as? [[String: Any]], !rows.isEmpty else { throw AlbumLookupError.invalidResponse("iTunes") }
        let album = rows.first { $0["wrapperType"] as? String == "collection" } ?? [:]
        let songs = rows.filter { $0["wrapperType"] as? String == "track" && $0["kind"] as? String != "music-video" }
        let counts = Dictionary(grouping: songs, by: { max(1, integer($0["discNumber"]) ?? 1) }).mapValues(\.count)
        let tracks = try validatedTracks(songs.enumerated().map { index, song in
            let disc = max(1, integer(song["discNumber"]) ?? 1)
            let duration = (song["trackTimeMillis"] as? NSNumber)?.doubleValue
            return AlbumLookupTrack(title: song["trackName"] as? String ?? "", artist: song["artistName"] as? String ?? release.artist,
                                    disc: disc, track: integer(song["trackNumber"]) ?? index + 1,
                                    trackTotal: integer(song["trackCount"]) ?? counts[disc] ?? songs.count,
                                    duration: duration.flatMap { $0 > 0 ? $0 / 1000 : nil })
        })
        let details = AlbumLookupDetails(title: album["collectionName"] as? String ?? release.title,
                                         artist: album["artistName"] as? String ?? release.artist,
                                         year: year(album["releaseDate"] as? String) ?? release.year,
                                         genre: album["primaryGenreName"] as? String, tracks: tracks, cover: nil, coverWarning: nil)
        return LookupPacket(details: details, coverURL: imageURL(album["artworkUrl100"] as? String) ?? release.coverURL)
    }
    nonisolated static func parseDeezerSearch(_ data: Data) throws -> [AlbumLookupRelease] {
        let object = try object(data, service: "Deezer")
        guard let rows = object["data"] as? [[String: Any]] else { throw AlbumLookupError.invalidResponse("Deezer") }
        var seen = Set<Int>()
        return rows.compactMap { item in
            let row = item["album"] as? [String: Any] ?? item
            guard let id = integer(row["id"]), id > 0, seen.insert(id).inserted,
                  let title = row["title"] as? String, !title.isEmpty else { return nil }
            let artist = (row["artist"] as? [String: Any] ?? item["artist"] as? [String: Any])?["name"] as? String ?? ""
            let source = (row["link"] as? String).flatMap(URL.init(string:)) ?? URL(string: "https://www.deezer.com/album/\(id)")!
            return AlbumLookupRelease(id: "deezer:\(id)", title: title, artist: artist,
                                      year: year(row["release_date"] as? String), country: nil, trackCount: integer(row["nb_tracks"]),
                                      sourceURL: source, provider: .deezer, providerID: String(id),
                                      coverURL: imageURL(row["cover_xl"] as? String ?? row["cover_big"] as? String))
        }
    }
    private static func parseDeezerDetails(_ album: [String: Any], release: AlbumLookupRelease) throws -> LookupPacket {
        let songs = (album["tracks"] as? [String: Any])?["data"] as? [[String: Any]] ?? []
        if let count = integer(album["nb_tracks"]), count > songs.count { throw AlbumLookupError.incompleteTracks }
        let counts = Dictionary(grouping: songs, by: { max(1, integer($0["disk_number"]) ?? 1) }).mapValues(\.count)
        var positions: [Int: Int] = [:]
        var tracks: [AlbumLookupTrack] = []
        for song in songs {
            let disc = max(1, integer(song["disk_number"]) ?? 1)
            positions[disc, default: 0] += 1
            let number = integer(song["track_position"]).flatMap { $0 > 0 ? $0 : nil } ?? positions[disc]!
            let duration = (song["duration"] as? NSNumber)?.doubleValue
            tracks.append(AlbumLookupTrack(title: song["title"] as? String ?? "", artist: (song["artist"] as? [String: Any])?["name"] as? String ?? release.artist,
                                           disc: disc, track: number, trackTotal: counts[disc] ?? songs.count,
                                           duration: duration.flatMap { $0 > 0 ? $0 : nil }))
        }
        let genres = (album["genres"] as? [String: Any])?["data"] as? [[String: Any]]
        let details = AlbumLookupDetails(title: album["title"] as? String ?? release.title,
                                         artist: (album["artist"] as? [String: Any])?["name"] as? String ?? release.artist,
                                         year: year(album["release_date"] as? String) ?? release.year,
                                         genre: genres?.first?["name"] as? String,
                                         tracks: try validatedTracks(tracks), cover: nil, coverWarning: nil)
        return LookupPacket(details: details, coverURL: imageURL(album["cover_xl"] as? String ?? album["cover_big"] as? String) ?? release.coverURL)
    }
    private static func validatedTracks(_ tracks: [AlbumLookupTrack]) throws -> [AlbumLookupTrack] {
        var seen = Set<String>()
        guard !tracks.isEmpty, tracks.allSatisfy({ !$0.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && $0.disc > 0 && $0.track > 0 && seen.insert("\($0.disc):\($0.track)").inserted }) else {
            throw AlbumLookupError.incompleteTracks
        }
        return tracks.sorted { ($0.disc, $0.track) < ($1.disc, $1.track) }
    }
    nonisolated static func broadSearchQuery(artist: String, album: String) throws -> String {
        let title = AlbumLookupMatching.normalized(album)
        guard !title.isEmpty else { throw AlbumLookupError.query("Enter an album title to search.") }
        let artist = AlbumLookupMatching.normalized(artist)
        return "release:(" + title + ")" + (artist.isEmpty ? "" : " AND artist:(" + artist + ")")
    }

    nonisolated static func searchQuery(artist: String, album: String) throws -> String {
        let artist = artist.trimmingCharacters(in: .whitespacesAndNewlines)
        let album = album.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !album.isEmpty else { throw AlbumLookupError.query("Enter an album title to search.") }
        guard artist.count <= 256, album.count <= 256 else {
            throw AlbumLookupError.query("Use an artist and album title shorter than 257 characters.")
        }
        func quoted(_ value: String) -> String {
            "\"" + value.replacingOccurrences(of: "\\", with: "\\\\")
                .replacingOccurrences(of: "\"", with: "\\\"") + "\""
        }
        var query = "release:\(quoted(album))"
        if !artist.isEmpty { query += " AND artist:\(quoted(artist))" }
        return query
    }

    nonisolated static func parseSearch(_ data: Data) throws -> [AlbumLookupRelease] {
        let response: SearchResponse
        do { response = try JSONDecoder().decode(SearchResponse.self, from: data) }
        catch { throw AlbumLookupError.invalidResponse("MusicBrainz") }
        var seen = Set<String>()
        return response.releases.compactMap { release in
            guard UUID(uuidString: release.id) != nil, !release.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                  seen.insert(release.id).inserted,
                  let sourceURL = URL(string: "https://musicbrainz.org/release/\(release.id)") else { return nil }
            let media = release.media ?? []
            let counts = media.compactMap(\.trackCount)
            let total = counts.isEmpty || counts.count != media.count ? nil : counts.reduce(0, +)
            return AlbumLookupRelease(id: release.id, title: release.title, artist: credit(release.artistCredit),
                                      year: year(release.date), country: release.country,
                                      trackCount: total, sourceURL: sourceURL)
        }
    }

    nonisolated static func parseDetails(_ data: Data) throws -> AlbumLookupDetails {
        let release: ReleaseResponse
        do { release = try JSONDecoder().decode(ReleaseResponse.self, from: data) }
        catch { throw AlbumLookupError.invalidResponse("MusicBrainz") }
        let artist = credit(release.artistCredit)
        var tracks: [AlbumLookupTrack] = []
        var positions = Set<String>()
        for (discIndex, medium) in (release.media ?? []).enumerated() {
            let disc = medium.position ?? discIndex + 1
            let songs = medium.tracks ?? []
            guard disc > 0, medium.trackCount == nil || medium.trackCount == songs.count else {
                throw AlbumLookupError.incompleteTracks
            }
            for (index, song) in songs.enumerated() {
                let number = song.position ?? index + 1
                let title = song.title ?? song.recording?.title ?? ""
                guard number > 0, !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                      positions.insert("\(disc):\(number)").inserted else {
                    throw AlbumLookupError.incompleteTracks
                }
                let songArtist = credit(song.artistCredit ?? song.recording?.artistCredit)
                let length = song.length ?? song.recording?.length
                tracks.append(AlbumLookupTrack(title: title, artist: songArtist.isEmpty ? artist : songArtist,
                                               disc: disc, track: number, trackTotal: songs.count,
                                               duration: length.flatMap { $0 > 0 ? Double($0) / 1000 : nil }))
            }
            guard Set(songs.enumerated().map { $0.element.position ?? $0.offset + 1 }) == Set(1..<(songs.count + 1)) else {
                throw AlbumLookupError.incompleteTracks
            }
        }
        guard !tracks.isEmpty, !release.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw AlbumLookupError.incompleteTracks
        }
        tracks.sort { ($0.disc, $0.track) < ($1.disc, $1.track) }
        let genre = release.genres?.filter { !$0.name.isEmpty }.sorted {
            if ($0.count ?? 0) != ($1.count ?? 0) { return ($0.count ?? 0) > ($1.count ?? 0) }
            return $0.name < $1.name
        }.first?.name
        return AlbumLookupDetails(title: release.title, artist: artist, year: year(release.date),
                                  genre: genre, tracks: tracks, cover: nil, coverWarning: nil)
    }

    nonisolated private static func credit(_ artists: [ArtistCredit]?) -> String {
        (artists ?? []).map { ($0.name ?? $0.artist?.name ?? "") + ($0.joinphrase ?? "") }.joined()
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    nonisolated private static func year(_ date: String?) -> Int? {
        guard let date, date.count >= 4, let value = Int(date.prefix(4)), value > 0 else { return nil }
        return value
    }

    nonisolated private static func validCover(_ data: Data) -> Bool {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
              let type = CGImageSourceGetType(source) as String?, ["public.jpeg", "public.png"].contains(type),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int,
              let height = properties[kCGImagePropertyPixelHeight] as? Int else { return false }
        return width > 0 && height > 0 && width <= 8192 && height <= 8192 && width * height <= 40_000_000
    }
}

private struct SearchResponse: Decodable {
    let releases: [ReleaseResponse]
}

private struct ReleaseResponse: Decodable {
    let id: String
    let title: String
    let date: String?
    let country: String?
    let artistCredit: [ArtistCredit]?
    let media: [MediumResponse]?
    let genres: [GenreResponse]?

    enum CodingKeys: String, CodingKey {
        case id, title, date, country, media, genres
        case artistCredit = "artist-credit"
    }
}

private struct ArtistCredit: Decodable {
    let name: String?
    let artist: ArtistResponse?
    let joinphrase: String?
}

private struct ArtistResponse: Decodable { let name: String? }
private struct GenreResponse: Decodable { let name: String; let count: Int? }

private struct MediumResponse: Decodable {
    let position: Int?
    let trackCount: Int?
    let tracks: [TrackResponse]?
    enum CodingKeys: String, CodingKey {
        case position, tracks
        case trackCount = "track-count"
    }
}

private struct TrackResponse: Decodable {
    let title: String?
    let position: Int?
    let length: Int?
    let artistCredit: [ArtistCredit]?
    let recording: RecordingResponse?
    enum CodingKeys: String, CodingKey {
        case title, position, length, recording
        case artistCredit = "artist-credit"
    }
}

private struct RecordingResponse: Decodable {
    let title: String?
    let length: Int?
    let artistCredit: [ArtistCredit]?
    enum CodingKeys: String, CodingKey {
        case title, length
        case artistCredit = "artist-credit"
    }
}

enum AlbumLookupMatching {
    static func replacing(_ text: String, _ pattern: String, _ replacement: String = "") -> String {
        text.replacingOccurrences(of: pattern, with: replacement, options: .regularExpression)
    }
    static func normalized(_ text: String) -> String {
        let folded = text.folding(options: [.diacriticInsensitive, .caseInsensitive, .widthInsensitive], locale: Locale(identifier: "en_US_POSIX"))
        return replacing(replacing(folded, "[^\\p{L}\\p{N}]+", " "), "\\s+", " ").trimmingCharacters(in: .whitespacesAndNewlines)
    }
    static func placeholder(_ text: String) -> Bool {
        let value = normalized(text)
        return value.isEmpty || ["unknown", "unknown album", "unknown artist", "music", "downloads", "audio", "untitled", "various", "various artists"].contains(value)
    }
    static func cleanFolder(_ text: String) -> String {
        var name = replacing(text, "(?i)\\s*[\\[(](?:flac|mp3|aac|alac|wav|ogg|opus|lossless|hi[ -]?res|\\d{2,3}\\s?kbps|\\d{2}[ -]?bit|\\d{2,3}[ .-]\\d{1,3})[^\\])]*[\\])]", "")
        name = replacing(name, "(?i)\\s*(?:[-–—]\\s*)?(?:cd|disc|disk)\\s*\\d+\\s*$", "")
        name = replacing(name, "(?i)\\s*[\\[(](?:[a-z]{2,6}[- ]?\\d{2,8}|eac|log|cue)[^\\])]*[\\])]", "")
        name = replacing(name, "(?<=\\S)\\s*[\\[(](?:19|20)\\d{2}[\\])]\\s*$", "")
        name = replacing(name, "^(?:19|20)\\d{2}\\s+[-–—]\\s+", "")
        return name.trimmingCharacters(in: .whitespacesAndNewlines)
    }
    static func cleanTrack(_ text: String) -> String {
        var value = text
        let ext = (value as NSString).pathExtension.lowercased()
        if ["mp3", "flac", "m4a", "wav", "ogg", "opus", "aif", "aiff", "oga", "m4b"].contains(ext) {
            value = (value as NSString).deletingPathExtension
        }
        value = replacing(value, "^\\s*(?:(?:cd|disc)\\s*)?\\d{1,2}[-._]\\d{1,3}\\s*[-._ )]+", "")
        value = replacing(value, "^\\s*\\d{1,3}\\s*[-._ )]+", "")
        return value.trimmingCharacters(in: .whitespacesAndNewlines)
    }
    static func distinctiveKeyword(_ album: String) -> String? {
        let words = normalized(album).split(separator: " ").map(String.init)
        guard words.count >= 3 else { return nil }
        let ignored: Set<String> = ["the", "and", "of", "a", "an", "ost", "original", "soundtrack", "music", "album", "edition", "deluxe", "remastered", "remaster", "volume", "vol", "complete", "collection"]
        return words.last { $0.count >= 6 && !ignored.contains($0) && !$0.allSatisfy(\.isNumber) }
    }
    static func filenamePosition(_ name: String) -> (disc: Int, track: Int)? {
        let patterns = ["(?i)^\\s*(?:(?:cd|disc)\\s*)?(\\d{1,2})[-._](\\d{1,3})(?:\\s|[-._])", "^\\s*(\\d{1,3})(?:\\s|[-._)])"]
        for (index, pattern) in patterns.enumerated() {
            guard let regex = try? NSRegularExpression(pattern: pattern),
                  let match = regex.firstMatch(in: name, range: NSRange(name.startIndex..., in: name)),
                  let firstRange = Range(match.range(at: 1), in: name), let first = Int(name[firstRange]) else { continue }
            if index == 0, let secondRange = Range(match.range(at: 2), in: name), let second = Int(name[secondRange]), first > 0, second > 0 { return (first, second) }
            if index == 1, first > 0 { return (1, first) }
        }
        return nil
    }
    static func localTitle(_ track: AlbumLookupLocalTrack, artist: String) -> String {
        var title = placeholder(track.title) ? cleanTrack(track.fileName) : cleanTrack(track.title)
        if !artist.isEmpty {
            let prefix = artist + " - "
            if title.lowercased().hasPrefix(prefix.lowercased()) { title = String(title.dropFirst(prefix.count)) }
        }
        return title
    }
    static func similarity(_ lhs: String, _ rhs: String) -> Double {
        let a = normalized(lhs), b = normalized(rhs)
        guard !a.isEmpty, !b.isEmpty else { return 0 }
        if a == b { return 1 }
        let x = Set(a.split(separator: " ")), y = Set(b.split(separator: " "))
        let tokens = Double(x.intersection(y).count) / Double(max(1, x.union(y).count))
        let initialsA = a.split(separator: " ").compactMap(\.first).map(String.init).joined()
        let initialsB = b.split(separator: " ").compactMap(\.first).map(String.init).joined()
        if (2...6).contains(initialsA.count), b.replacingOccurrences(of: " ", with: "") == initialsA { return 0.82 }
        if (2...6).contains(initialsB.count), a.replacingOccurrences(of: " ", with: "") == initialsB { return 0.82 }
        if min(a.count, b.count) >= 6 && (x.isSubset(of: y) || y.isSubset(of: x)) {
            return max(tokens, 0.75 + 0.25 * Double(min(x.count, y.count)) / Double(max(x.count, y.count)))
        }
        if a.contains(b) || b.contains(a) { return max(tokens, Double(min(a.count, b.count)) / Double(max(a.count, b.count))) }
        return tokens
    }
    static func precedes(_ a: AlbumLookupRelease, _ b: AlbumLookupRelease) -> Bool {
        if a.matchScore != b.matchScore { return (a.matchScore ?? 0) > (b.matchScore ?? 0) }
        if a.provider != b.provider { return a.provider.rawValue < b.provider.rawValue }
        return a.id < b.id
    }
    static func rank(_ release: AlbumLookupRelease, query: AlbumLookupQuery, tracks: [AlbumLookupTrack]? = nil) -> AlbumLookupRelease {
        let folder = AlbumLookupQuery(folderName: query.folderName, tracks: query.tracks).inferred
        let albumNames = [query.album, folder.album].filter { !$0.isEmpty }
        let artistNames = [query.artist, folder.artist].filter { !$0.isEmpty }
        var score = 0.0, weight = 0.0
        if !albumNames.isEmpty {
            score += 35 * (albumNames.map { similarity(release.title, $0) }.max() ?? 0)
            weight += 35
        }
        if !artistNames.isEmpty {
            score += 15 * (artistNames.map { similarity(release.artist, $0) }.max() ?? 0)
            weight += 15
        }
        let sourceCount = tracks?.count ?? release.trackCount
        var summary: [String] = []
        if !query.tracks.isEmpty, let count = sourceCount {
            let difference = abs(count - query.tracks.count)
            let agreement = difference == 0 ? 1 : max(0, 1 - Double(difference) / Double(max(1, query.tracks.count)) * 2)
            score += 15 * agreement
            weight += 15
            summary.append("\(count) of \(query.tracks.count) tracks")
        }
        if let source = tracks, !query.tracks.isEmpty {
            var total = 0.0, checked = 0, matches = 0
            for local in query.tracks {
                let names = [localTitle(local, artist: query.artist), localTitle(AlbumLookupLocalTrack(fileName: local.fileName, title: ""), artist: folder.artist.isEmpty ? query.artist : folder.artist)]
                    .filter { !$0.isEmpty && normalized($0).range(of: "^(track|audio|song) ?[0-9]+$", options: .regularExpression) == nil }
                guard !names.isEmpty else { continue }
                let best = source.map { track in names.map { similarity($0, track.title) }.max() ?? 0 }.max() ?? 0
                total += best
                checked += 1
                if best >= 0.8 { matches += 1 }
            }
            if checked > 0 {
                score += 35 * total / Double(checked)
                weight += 35
                summary.append("\(matches) titles match")
            }
            let numbered = query.tracks.filter { $0.track > 0 }
            if !numbered.isEmpty {
                let positions = Set(source.map { "\($0.disc):\($0.track)" })
                let matched = numbered.filter { positions.contains("\(max(1, $0.disc)):\($0.track)") }.count
                score += 5 * Double(matched) / Double(numbered.count)
                weight += 5
            }
        }
        var ranked = release
        let result = weight == 0 ? 0 : score / weight * 100
        ranked.matchScore = Int((min(100, result) * (tracks == nil ? 0.78 : 1)).rounded())
        ranked.matchSummary = summary.isEmpty ? "Compared album and artist names" : summary.joined(separator: " · ")
        return ranked
    }
}
