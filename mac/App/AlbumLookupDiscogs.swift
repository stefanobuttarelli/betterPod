// Discogs releases as a lookup source. Searches prefer CD editions, whose
// numbering and lengths match ripped files; other formats are a fallback.
// Results stay in memory until the editor applies them.
import Foundation

enum DiscogsAlbums {
    static func searchItems(album: String, artist: String, compactDiscOnly: Bool) -> [URLQueryItem] {
        var items = [URLQueryItem(name: "type", value: "release"), URLQueryItem(name: "release_title", value: album),
                     URLQueryItem(name: "per_page", value: "10")]
        if !artist.isEmpty { items.append(URLQueryItem(name: "artist", value: artist)) }
        if compactDiscOnly { items.append(URLQueryItem(name: "format", value: "CD")) }
        return items
    }

    /// One edition per album: the first result of each master release.
    static func parseSearch(_ data: Data) throws -> [AlbumLookupRelease] {
        let object = try object(data)
        guard let rows = object["results"] as? [[String: Any]] else { throw AlbumLookupError.invalidResponse("Discogs") }
        var albums = Set<Int>()
        return rows.compactMap { row in
            guard let id = row["id"] as? Int, id > 0, (row["type"] as? String ?? "release") == "release",
                  let name = row["title"] as? String, !name.isEmpty else { return nil }
            let master = row["master_id"] as? Int ?? 0
            guard albums.insert(master > 0 ? master : -id).inserted else { return nil }
            let parts = name.components(separatedBy: " - ")
            let artist = parts.count > 1 ? cleanArtist(parts[0]) : ""
            let title = parts.count > 1 ? parts.dropFirst().joined(separator: " - ") : name
            let source = (row["uri"] as? String).flatMap { URL(string: "https://www.discogs.com" + $0) }
                ?? URL(string: "https://www.discogs.com/release/\(id)")!
            let formats = (row["format"] as? [String] ?? []).prefix(2).joined(separator: ", ")
            return AlbumLookupRelease(id: "discogs:release:\(id)", title: title, artist: artist,
                                      year: (row["year"] as? String).flatMap { Int($0) }.flatMap { $0 > 0 ? $0 : nil },
                                      country: row["country"] as? String, trackCount: nil, sourceURL: source,
                                      provider: .discogs, providerID: "release:\(id)", coverURL: imageURL(row["cover_image"] as? String),
                                      sourceLabel: formats.isEmpty ? nil : "Discogs · \(formats)")
        }
    }

    static func detailsPath(_ providerID: String) -> String? {
        let parts = providerID.split(separator: ":")
        guard parts.count == 2, ["release", "master"].contains(parts[0]), let id = Int(parts[1]), id > 0 else { return nil }
        return "/\(parts[0])s/\(id)"
    }

    static func parseDetails(_ data: Data) throws -> (details: AlbumLookupDetails, coverURL: URL?) {
        let object = try object(data)
        guard let title = object["title"] as? String, !title.trimmingCharacters(in: .whitespaces).isEmpty else {
            throw AlbumLookupError.invalidResponse("Discogs")
        }
        let artist = credit(object["artists"] as? [[String: Any]])
        var year = (object["year"] as? Int).flatMap { $0 > 0 ? $0 : nil }
        if year == nil, let released = object["released"] as? String, released.count >= 4 { year = Int(released.prefix(4)) }
        // Several broad genres say little ("Electronic, Rock"); the first style is then more precise.
        let genres = object["genres"] as? [String] ?? []
        let styles = object["styles"] as? [String] ?? []
        let genre = genres.count > 1 ? styles.first ?? genres.first : genres.first ?? styles.first
        var entries: [[String: Any]] = []
        for entry in object["tracklist"] as? [[String: Any]] ?? [] {
            switch entry["type_"] as? String ?? "track" {
            case "track": entries.append(entry)
            case "index": entries += entry["sub_tracks"] as? [[String: Any]] ?? []
            default: continue
            }
        }
        let tracks = numbered(entries.filter { ($0["type_"] as? String ?? "track") == "track" }, artist: artist)
        let images = object["images"] as? [[String: Any]] ?? []
        let image = images.first { $0["type"] as? String == "primary" } ?? images.first
        return (AlbumLookupDetails(title: title, artist: artist, year: year, genre: genre, tracks: tracks, cover: nil, coverWarning: nil),
                imageURL(image?["uri"] as? String))
    }

    /// Positions such as "1-03" or "CD2-1" give discs; plain numbers keep their
    /// order; vinyl sides ("A1", "B2") and missing positions count from one.
    private static func numbered(_ entries: [[String: Any]], artist: String) -> [AlbumLookupTrack] {
        let titles = entries.map { ($0["title"] as? String ?? "").trimmingCharacters(in: .whitespacesAndNewlines) }
        guard !entries.isEmpty, titles.allSatisfy({ !$0.isEmpty }) else { return [] }
        let positions = entries.map { ($0["position"] as? String ?? "").trimmingCharacters(in: .whitespaces) }
        var numbers: [(disc: Int, track: Int)] = positions.map { position in
            guard let match = position.range(of: #"(?i)^(?:cd|disc|disk)?\s*(\d{1,2})\s*[-.]\s*(\d{1,3})$"#, options: .regularExpression) else { return (0, 0) }
            let digits = position[match].split { !$0.isNumber }.compactMap { Int($0) }
            return digits.count == 2 ? (digits[0], digits[1]) : (0, 0)
        }
        if numbers.contains(where: { $0.disc <= 0 || $0.track <= 0 }) {
            let plain = positions.compactMap { Int($0) }
            numbers = plain.count == positions.count && Set(plain).count == plain.count && plain.allSatisfy({ $0 > 0 })
                ? plain.map { (1, $0) } : positions.indices.map { (1, $0 + 1) }
        }
        guard Set(numbers.map { "\($0.disc):\($0.track)" }).count == numbers.count else { return [] }
        let totals = Dictionary(grouping: numbers, by: \.disc).mapValues(\.count)
        return entries.indices.map { index in
            let trackArtist = credit(entries[index]["artists"] as? [[String: Any]])
            return AlbumLookupTrack(title: titles[index], artist: trackArtist.isEmpty ? artist : trackArtist,
                                    disc: numbers[index].disc, track: numbers[index].track,
                                    trackTotal: totals[numbers[index].disc] ?? entries.count,
                                    duration: (entries[index]["duration"] as? String).flatMap(WikipediaAlbums.duration))
        }.sorted { ($0.disc, $0.track) < ($1.disc, $1.track) }
    }

    /// A Discogs release or master link pasted into the search, for example
    /// discogs.com/it/release/2621910-DJ-Gruff-Rapadopa.
    static func link(in text: String) -> String? {
        guard let range = text.range(of: #"(?i)discogs\.com/(?:[a-z]{2}(?:-[a-z]{2})?/)?(?:[^/\s]+/)?(release|master)/(\d+)"#, options: .regularExpression) else { return nil }
        let path = text[range].lowercased()
        let kind = path.contains("/master/") ? "master" : "release"
        guard let id = path.split(separator: "/").last.flatMap({ Int($0.prefix { $0.isNumber }) }), id > 0 else { return nil }
        return "\(kind):\(id)"
    }

    private static func credit(_ artists: [[String: Any]]?) -> String {
        let artists = artists ?? []
        var text = ""
        for (index, artist) in artists.enumerated() {
            let variation = (artist["anv"] as? String ?? "").trimmingCharacters(in: .whitespaces)
            text += cleanArtist(variation.isEmpty ? artist["name"] as? String ?? "" : variation)
            guard index < artists.count - 1 else { break }
            let join = (artist["join"] as? String ?? "").trimmingCharacters(in: .whitespaces)
            text += join.isEmpty || join == "," ? ", " : " \(join) "
        }
        return text.trimmingCharacters(in: .whitespaces)
    }

    /// Discogs numbers artists who share a name ("Nirvana (2)") and marks name variations with "*".
    static func cleanArtist(_ name: String) -> String {
        name.replacingOccurrences(of: #"\s*\(\d+\)$"#, with: "", options: .regularExpression)
            .trimmingCharacters(in: CharacterSet(charactersIn: "* ").union(.whitespaces))
    }

    private static func imageURL(_ text: String?) -> URL? {
        guard let text, let url = URL(string: text), url.scheme == "https",
              let host = url.host?.lowercased(), host == "i.discogs.com" || host.hasSuffix(".discogs.com"),
              !url.path.hasSuffix(".gif") else { return nil }
        return url
    }

    private static func object(_ data: Data) throws -> [String: Any] {
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AlbumLookupError.invalidResponse("Discogs")
        }
        if let message = object["message"] as? String, object["results"] == nil, object["title"] == nil {
            throw AlbumLookupError.service("Discogs", String(message.prefix(256)))
        }
        return object
    }
}

// Discogs allows 25 requests a minute without an account. Leave a margin,
// fail this provider promptly, and keep the other services available.
actor DiscogsRequestBudget {
    static let shared = DiscogsRequestBudget()
    private var requests: [ContinuousClock.Instant] = []
    func reserve() throws {
        let now = ContinuousClock.now
        requests.removeAll { $0.duration(to: now) >= .seconds(60) }
        guard requests.count < 22 else {
            throw AlbumLookupError.service("Discogs", "The local request limit was reached. Wait a minute or use another service.")
        }
        requests.append(now)
    }
}
