// Wikipedia album articles as a lookup source. The album infobox and track
// listing are read from each article's wikitext. Results stay in memory until
// the editor applies them; nothing here reads or writes music files.
import Foundation

struct WikipediaEdition: Sendable, Hashable {
    let code: String
    /// The album infobox template, used to keep searches to album articles.
    let infobox: String
    let label: String

    static let english = WikipediaEdition(code: "en", infobox: "Infobox album", label: "Wikipedia")
    static let italian = WikipediaEdition(code: "it", infobox: "Album", label: "Wikipedia (it)")
    static let all = [english, italian]
}

struct WikipediaAlbum: Sendable {
    let release: AlbumLookupRelease
    let details: AlbumLookupDetails
    let coverURL: URL?
}

enum WikipediaAlbums {
    private static let infoboxNames: Set<String> = ["infobox album", "album"]
    private static let trackListNames: Set<String> = ["track listing", "tracklist", "track list", "tracce"]
    private static let titleKeys = ["name", "titolo"]
    private static let artistKeys = ["artist", "artista"]
    private static let dateKeys = ["released", "anno"]
    private static let genreKeys = ["genre", "genere"]
    private static let coverKeys = ["cover", "immagine"]

    static func searchItems(album: String, artist: String, edition: WikipediaEdition) -> [URLQueryItem] {
        // Quotes, colons and wildcards are search syntax. Album names only need plain words.
        let words = [album, artist].joined(separator: " ")
            .replacingOccurrences(of: #"["*?~:\\]"#, with: " ", options: .regularExpression)
            .split(whereSeparator: \.isWhitespace).joined(separator: " ")
        return [
            URLQueryItem(name: "action", value: "query"), URLQueryItem(name: "format", value: "json"),
            URLQueryItem(name: "formatversion", value: "2"), URLQueryItem(name: "generator", value: "search"),
            URLQueryItem(name: "gsrsearch", value: words + " hastemplate:\"" + edition.infobox + "\""),
            URLQueryItem(name: "gsrnamespace", value: "0"), URLQueryItem(name: "gsrlimit", value: "5")
        ] + pageItems
    }

    static func pageItems(pageID: Int) -> [URLQueryItem] {
        [URLQueryItem(name: "action", value: "query"), URLQueryItem(name: "format", value: "json"),
         URLQueryItem(name: "formatversion", value: "2"), URLQueryItem(name: "pageids", value: String(pageID))] + pageItems
    }

    private static let pageItems = [
        URLQueryItem(name: "prop", value: "revisions|pageimages|info"), URLQueryItem(name: "rvprop", value: "content"),
        URLQueryItem(name: "rvslots", value: "main"), URLQueryItem(name: "piprop", value: "thumbnail|name"),
        URLQueryItem(name: "pithumbsize", value: "1200"), URLQueryItem(name: "pilicense", value: "any"),
        URLQueryItem(name: "inprop", value: "url")
    ]

    /// Album articles in search order. Pages without an album infobox are left out.
    static func parse(_ data: Data, edition: WikipediaEdition) throws -> [WikipediaAlbum] {
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AlbumLookupError.invalidResponse("Wikipedia")
        }
        if let error = object["error"] as? [String: Any] {
            let message = error["info"] as? String ?? "The service rejected the request."
            throw AlbumLookupError.service("Wikipedia", String(message.prefix(256)))
        }
        let pages = (object["query"] as? [String: Any])?["pages"] as? [[String: Any]] ?? []
        return pages.sorted { ($0["index"] as? Int ?? 0) < ($1["index"] as? Int ?? 0) }
            .compactMap { album(from: $0, edition: edition) }
    }

    private static func album(from page: [String: Any], edition: WikipediaEdition) -> WikipediaAlbum? {
        guard let pageID = page["pageid"] as? Int, pageID > 0,
              let revision = (page["revisions"] as? [[String: Any]])?.first,
              let content = ((revision["slots"] as? [String: Any])?["main"] as? [String: Any])?["content"] as? String
                ?? revision["content"] as? String else { return nil }
        let text = WikiText.cleaned(content)
        guard let infobox = WikiText.templates(in: text).first(where: { infoboxNames.contains($0.name) }) else { return nil }
        let pageTitle = page["title"] as? String ?? ""
        var title = WikiText.plain(infobox.value(titleKeys))
        if title.isEmpty { title = WikiText.replacing(pageTitle, #"(?i)\s*\([^()]*\balbum\b[^()]*\)$"#, "") }
        guard !title.isEmpty else { return nil }
        let artist = WikiText.plain(infobox.value(artistKeys))
        let year = firstYear(infobox.value(dateKeys))
        let genre = firstGenre(infobox.value(genreKeys))
        let tracks = self.tracks(in: text, artist: artist)
        var coverURL: URL?
        let cover = fileName(infobox.value(coverKeys))
        if !cover.isEmpty, fileName(page["pageimage"] as? String ?? "") == cover,
           let source = ((page["thumbnail"] as? [String: Any])?["source"] as? String).flatMap(URL.init(string:)),
           source.scheme == "https" {
            coverURL = source
        }
        let link = (page["fullurl"] as? String).flatMap(URL.init(string:))
            ?? URL(string: "https://\(edition.code).wikipedia.org/?curid=\(pageID)")!
        let release = AlbumLookupRelease(id: "wikipedia:\(edition.code):\(pageID)", title: title, artist: artist, year: year,
                                         country: nil, trackCount: tracks.isEmpty ? nil : tracks.count, sourceURL: link,
                                         provider: .wikipedia, providerID: "\(edition.code):\(pageID)", coverURL: coverURL,
                                         sourceLabel: edition.label)
        let details = AlbumLookupDetails(title: title, artist: artist, year: year, genre: genre, tracks: tracks,
                                         cover: nil, coverWarning: nil)
        return WikipediaAlbum(release: release, details: details, coverURL: coverURL)
    }

    static func firstYear(_ raw: String) -> Int? {
        let text = WikiText.plain(raw)
        guard let range = text.range(of: #"(?<!\d)(1[89]\d{2}|20\d{2})(?!\d)"#, options: .regularExpression) else { return nil }
        return Int(text[range])
    }

    static func firstGenre(_ raw: String) -> String? {
        let first = WikiText.plain(raw).components(separatedBy: CharacterSet(charactersIn: ",/·•;"))
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.first { !$0.isEmpty }
        guard let first else { return nil }
        return first.prefix(1).uppercased() + first.dropFirst()
    }

    /// A file name as the infobox and the page image name it: without the
    /// namespace, link markup or underscores, case-insensitive.
    static func fileName(_ raw: String) -> String {
        var name = raw.replacingOccurrences(of: "[[", with: "").replacingOccurrences(of: "]]", with: "")
        name = name.components(separatedBy: "|").first ?? ""
        name = WikiText.replacing(name, #"(?i)^\s*(?:file|image|immagine)\s*:"#, "")
        return name.replacingOccurrences(of: "_", with: " ").trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    }

    // MARK: Track listings

    private struct ListedTrack {
        let disc: Int
        let track: Int
        let title: String
        let artist: String
        let duration: TimeInterval?
    }

    private struct ListedRow {
        let number: Int?
        let title: String
        let artist: String
        let duration: TimeInterval?
    }

    /// One track list and the labels above it, such as "Disc two", "Side B"
    /// or "Vinyl edition".
    private struct TrackBlock {
        let label: String
        var rows: [ListedRow] = []
    }

    /// Track listing templates first, then numbered lists under a track
    /// listing heading. Only the first edition is used. An unusable list
    /// gives no tracks, so album fields can still be applied.
    static func tracks(in text: String, artist: String) -> [AlbumLookupTrack] {
        var listed = assemble(templateBlocks(in: text, artist: artist))
        if listed.isEmpty { listed = assemble(listBlocks(in: text, artist: artist)) }
        var positions = Set<String>()
        guard !listed.isEmpty, listed.allSatisfy({ !$0.title.isEmpty && $0.disc > 0 && $0.track > 0 && positions.insert("\($0.disc):\($0.track)").inserted }) else {
            return []
        }
        let totals = Dictionary(grouping: listed, by: \.disc).mapValues(\.count)
        return listed.sorted { ($0.disc, $0.track) < ($1.disc, $1.track) }.map {
            AlbumLookupTrack(title: $0.title, artist: $0.artist, disc: $0.disc, track: $0.track,
                             trackTotal: totals[$0.disc] ?? listed.count, duration: $0.duration)
        }
    }

    /// The main edition's tracks. A disc label starts that disc, a side
    /// continues the current one, and an unlabeled list that restarts at one
    /// is the next disc. Once tracks are found, the next bonus or alternative
    /// edition ends the list.
    private static func assemble(_ blocks: [TrackBlock]) -> [ListedTrack] {
        var result: [ListedTrack] = []
        var disc = 1
        var last: [Int: Int] = [:]
        for block in blocks {
            if result.isEmpty {
                if isBonus(block.label) { continue }
            } else if isAlternative(block.label) {
                break
            }
            guard !block.rows.isEmpty else { continue }
            let first = block.rows.first?.number ?? 1
            if let number = discNumber(block.label) { disc = number }
            else if !result.isEmpty && !isSide(block.label) && first == 1 && (last[disc] ?? 0) > 0 { disc += 1 }
            // Vinyl sides restart at one in some articles; they continue the same disc.
            let offset = first == 1 ? last[disc] ?? 0 : 0
            for (index, row) in block.rows.enumerated() {
                let track = (row.number ?? first + index) + offset
                result.append(ListedTrack(disc: disc, track: track, title: row.title, artist: row.artist, duration: row.duration))
                last[disc] = max(last[disc] ?? 0, track)
            }
        }
        return result
    }

    private static func templateBlocks(in text: String, artist: String) -> [TrackBlock] {
        let chars = Array(text)
        var blocks: [TrackBlock] = []
        var previousEnd = 0
        for span in WikiText.spans(in: chars) where trackListNames.contains(span.template.name) {
            let list = span.template
            var labels = labelLines(String(chars[min(previousEnd, span.start)..<span.start]))
            let headline = WikiText.plain(list.named["headline"] ?? "")
            if !headline.isEmpty { labels.append(headline) }
            previousEnd = span.end
            var titles: [Int: String] = [:], lengths: [Int: String] = [:], extras: [Int: String] = [:]
            for (key, value) in list.named {
                guard key.range(of: #"^(title|titolo|length|durata|extra)\d+$"#, options: .regularExpression) != nil,
                      let number = Int(key.drop { !$0.isNumber }), number > 0, number < 1000 else { continue }
                if key.hasPrefix("title") || key.hasPrefix("titolo") { titles[number] = WikiText.plain(value) }
                else if key.hasPrefix("extra") { extras[number] = WikiText.plain(value) }
                else { lengths[number] = value }
            }
            let extraIsArtist = WikiText.plain(list.named["extra_column"] ?? "")
                .range(of: #"(?i)artist|performer|interprete"#, options: .regularExpression) != nil
            let rows = titles.keys.sorted().map { number -> ListedRow in
                let trackArtist = extraIsArtist ? extras[number] ?? "" : ""
                return ListedRow(number: number, title: titles[number] ?? "", artist: trackArtist.isEmpty ? artist : trackArtist,
                                 duration: lengths[number].flatMap(duration))
            }
            blocks.append(TrackBlock(label: labels.joined(separator: " · "), rows: rows))
        }
        return blocks
    }

    /// Labels written between two lists: sub-headings, ";term" lines and
    /// bold lines, from the current section only.
    private static func labelLines(_ gap: String) -> [String] {
        var labels: [String] = []
        for line in gap.components(separatedBy: .newlines) {
            let line = line.trimmingCharacters(in: .whitespaces)
            if let heading = heading(line) {
                if heading.level <= 2 { labels = [] }
                labels.append(heading.name)
            } else if let label = label(line) {
                labels.append(label)
            }
        }
        return labels
    }

    private static func label(_ line: String) -> String? {
        if line.hasPrefix(";") { return WikiText.plain(String(line.dropFirst())) }
        if line.hasPrefix(") && line.hasSuffix(") && line.count > 6 { return WikiText.plain(line) }
        return nil
    }

    private static func listBlocks(in text: String, artist: String) -> [TrackBlock] {
        var blocks: [TrackBlock] = []
        var sectionLevel: Int?
        for line in text.components(separatedBy: .newlines) {
            let line = line.trimmingCharacters(in: .whitespaces)
            if let heading = heading(line) {
                if let level = sectionLevel {
                    if heading.level <= level { break }
                    blocks.append(TrackBlock(label: heading.name))
                } else if heading.name.range(of: #"(?i)^(track ?listing|track ?list|tracks|tracce|lista tracce|tracce del disco|brani)$"#, options: .regularExpression) != nil {
                    sectionLevel = heading.level
                    blocks.append(TrackBlock(label: ""))
                }
                continue
            }
            guard sectionLevel != nil else { continue }
            if let label = label(line) {
                // A label under a sub-heading belongs to it, as "Vinyl edition · Side A".
                let parent = blocks.last.map { $0.rows.isEmpty ? $0.label : "" } ?? ""
                if blocks.last?.rows.isEmpty == true { blocks.removeLast() }
                blocks.append(TrackBlock(label: [parent, label].filter { !$0.isEmpty }.joined(separator: " · ")))
            } else if line.hasPrefix("#") && !line.hasPrefix("##") && !line.hasPrefix("#:") && !line.hasPrefix("#*") {
                let entry = String(line.dropFirst())
                let title = listedTitle(entry)
                guard !title.isEmpty else { continue }
                let time = entry.range(of: #"\d{1,2}:\d{2}(?::\d{2})?"#, options: [.regularExpression, .backwards]).map { String(entry[$0]) }
                if blocks.isEmpty { blocks.append(TrackBlock(label: "")) }
                blocks[blocks.count - 1].rows.append(ListedRow(number: nil, title: title, artist: artist, duration: time.flatMap(duration)))
            }
        }
        return blocks
    }

    private static func heading(_ line: String) -> (level: Int, name: String)? {
        guard line.hasPrefix("=="), let match = line.range(of: #"^(=+)\s*(.*?)\s*\1$"#, options: .regularExpression) else { return nil }
        let level = line.prefix { $0 == "=" }.count
        let inner = line[match].dropFirst(level).dropLast(level)
        return (level, WikiText.plain(String(inner)))
    }

    private static func listedTitle(_ entry: String) -> String {
        for pattern in [#""([^"]+)""#, #"“([^”]+)”"#, #"''(.+?)''"#] {
            if let range = entry.range(of: pattern, options: .regularExpression) {
                let title = WikiText.plain(String(entry[range]).trimmingCharacters(in: CharacterSet(charactersIn: "\"“”'")))
                if !title.isEmpty { return title }
            }
        }
        let first = entry.components(separatedBy: " – ").first?.components(separatedBy: " — ").first?
            .components(separatedBy: " - ").first ?? entry
        return WikiText.plain(first)
    }

    private static func discNumber(_ text: String) -> Int? {
        let words = ["one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6, "uno": 1, "due": 2, "tre": 3, "quattro": 4, "cinque": 5]
        guard let range = text.range(of: #"(?i)\b(?:disc|disk|cd|disco)\s*(\d{1,2}|one|two|three|four|five|six|uno|due|tre|quattro|cinque)\b"#, options: .regularExpression) else { return nil }
        let found = String(text[range])
        if let number = Int(found.filter(\.isNumber)) { return number }
        return found.split(whereSeparator: \.isWhitespace).last.flatMap { words[$0.lowercased()] }
    }

    private static func isSide(_ text: String) -> Bool {
        text.range(of: #"(?i)\b(?:side|lato|facciata)\b"#, options: .regularExpression) != nil
    }

    /// Material that is never the main list.
    private static func isBonus(_ text: String) -> Bool {
        text.range(of: #"(?i)bonus|\bdvd\b|\bvideo\b|\bextra\b"#, options: .regularExpression) != nil
    }

    /// Another edition of the album, or its bonus material.
    private static func isAlternative(_ text: String) -> Bool {
        isBonus(text) || text.range(of: #"(?i)edition|deluxe|reissue|re-issue|expanded|anniversary|remaster|japan|limited|digital|itunes|vinyl|cassette|\brelease\b|pressing|version|edizione|ristampa|vinile|musicassetta|versione"#, options: .regularExpression) != nil
    }

    static func duration(_ text: String) -> TimeInterval? {
        let parts = WikiText.plain(text).trimmingCharacters(in: .whitespaces).split(separator: ":").map { Int($0) }
        guard (2...3).contains(parts.count), parts.allSatisfy({ $0 != nil }) else { return nil }
        let value = parts.compactMap { $0 }.reduce(0) { $0 * 60 + $1 }
        return value > 0 ? TimeInterval(value) : nil
    }
}

// MARK: - Wikitext

struct WikiTemplate {
    let name: String
    let positional: [String]
    let named: [String: String]

    func value(_ keys: [String]) -> String {
        keys.lazy.compactMap { self.named[$0] }.first { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty } ?? ""
    }
}

enum WikiText {
    static func replacing(_ text: String, _ pattern: String, _ replacement: String) -> String {
        text.replacingOccurrences(of: pattern, with: replacement, options: .regularExpression)
    }

    /// Comments and references hold markup and pipes that are not part of the values.
    static func cleaned(_ text: String) -> String {
        var value = replacing(text, #"<!--[\s\S]*?-->"#, "")
        value = replacing(value, #"(?i)<ref[^>]*/>"#, "")
        value = replacing(value, #"(?i)<ref[^>]*>[\s\S]*?</ref>"#, "")
        return value
    }

    /// Every template in document order, including nested ones.
    static func templates(in text: String) -> [WikiTemplate] {
        spans(in: Array(text)).map(\.template)
    }

    /// Templates with their character offsets, from "{{" to just after "}}".
    static func spans(in chars: [Character]) -> [(template: WikiTemplate, start: Int, end: Int)] {
        var result: [(template: WikiTemplate, start: Int, end: Int)] = []
        var index = 0
        while index + 1 < chars.count {
            if chars[index] == "{" && chars[index + 1] == "{", let end = closing(chars, from: index + 2) {
                result.append((template(chars[(index + 2)..<(end - 2)]), index, end))
                index += 2
            } else {
                index += 1
            }
        }
        return result
    }

    /// Readable text: links keep their labels, list templates keep their
    /// items, and other templates, tags and formatting are removed.
    static func plain(_ text: String) -> String {
        var value = flatten(cleaned(text))
        value = replacing(value, #"(?i)\[\[(?:file|image|immagine|category|categoria):[^\[\]]*(?:\[\[[^\[\]]*\]\][^\[\]]*)*\]\]"#, "")
        value = replacing(value, #"\[\[(?:[^\[\]|]*\|)?([^\[\]]*)\]\]"#, "$1")
        value = replacing(value, #"\[https?://[^\s\]]+\s+([^\]]+)\]"#, "$1")
        value = replacing(value, #"\[https?://[^\]]*\]"#, "")
        value = replacing(value, #"'{2,}"#, "")
        value = replacing(value, #"(?i)<br\s*/?>"#, ", ")
        value = replacing(value, #"<[^>]+>"#, "")
        value = replacing(value, #"(?m)^\s*[*#]+\s*"#, ", ")
        for (entity, character) in ["&nbsp;": " ", "&amp;": "&", "&ndash;": "–", "&mdash;": "—", "&quot;": "\"", "&#39;": "'", "&apos;": "'"] {
            value = value.replacingOccurrences(of: entity, with: character)
        }
        value = replacing(value, #"\s+"#, " ")
        value = replacing(value, #"(?:\s*,\s*){2,}"#, ", ")
        return value.trimmingCharacters(in: CharacterSet(charactersIn: ", ").union(.whitespacesAndNewlines))
    }

    private static func flatten(_ text: String) -> String {
        guard text.contains("{{") else { return text }
        let chars = Array(text)
        var output = ""
        var index = 0
        while index < chars.count {
            if index + 1 < chars.count, chars[index] == "{", chars[index + 1] == "{", let end = closing(chars, from: index + 2) {
                output += readable(template(chars[(index + 2)..<(end - 2)]))
                index = end
            } else {
                output.append(chars[index])
                index += 1
            }
        }
        return output
    }

    private static func readable(_ template: WikiTemplate) -> String {
        let items = template.positional.map(flatten).map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
        switch template.name {
        case "hlist", "flatlist", "plainlist", "ubl", "unbulleted list", "flat list", "plain list":
            return items.joined(separator: ", ")
        case "nowrap", "small", "nobold", "sic", "nihongo", "nihongo2", "nihongo3", "korean", "smallcaps":
            return items.first ?? ""
        case "start date", "release date", "start date and age", "film date", "dts", "date":
            return items.joined(separator: " ")
        default:
            if template.name == "lang" || template.name.hasPrefix("lang-") { return items.last ?? "" }
            return ""
        }
    }

    /// The index just after the "}}" that closes a template opened before `start`.
    private static func closing(_ chars: [Character], from start: Int) -> Int? {
        var depth = 1
        var index = start
        while index + 1 < chars.count {
            if chars[index] == "{" && chars[index + 1] == "{" { depth += 1; index += 2; continue }
            if chars[index] == "}" && chars[index + 1] == "}" {
                depth -= 1
                index += 2
                if depth == 0 { return index }
                continue
            }
            index += 1
        }
        return nil
    }

    private static func template(_ body: ArraySlice<Character>) -> WikiTemplate {
        var parts: [String] = []
        var current = ""
        var braces = 0, links = 0
        var index = body.startIndex
        while index < body.endIndex {
            let char = body[index]
            let next = index + 1 < body.endIndex ? body[index + 1] : nil
            if char == "{" && next == "{" { braces += 1; current += "{{"; index += 2; continue }
            if char == "}" && next == "}" { braces -= 1; current += "}}"; index += 2; continue }
            if char == "[" && next == "[" { links += 1; current += "[["; index += 2; continue }
            if char == "]" && next == "]" { links -= 1; current += "]]"; index += 2; continue }
            if char == "|" && braces <= 0 && links <= 0 {
                parts.append(current)
                current = ""
            } else {
                current.append(char)
            }
            index += 1
        }
        parts.append(current)
        let name = replacing(parts[0].replacingOccurrences(of: "_", with: " "), #"(?i)^\s*template\s*:"#, "")
            .split(whereSeparator: \.isWhitespace).joined(separator: " ").lowercased()
        var positional: [String] = []
        var named: [String: String] = [:]
        for part in parts.dropFirst() {
            if let equals = topLevelEquals(part) {
                let key = part[..<equals].trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
                named[key] = String(part[part.index(after: equals)...]).trimmingCharacters(in: .whitespacesAndNewlines)
            } else {
                positional.append(part)
            }
        }
        return WikiTemplate(name: name, positional: positional, named: named)
    }

    private static func topLevelEquals(_ part: String) -> String.Index? {
        var depth = 0
        var previous: Character?
        for index in part.indices {
            let char = part[index]
            if (char == "{" || char == "[") && previous == char { depth += 1 }
            if (char == "}" || char == "]") && previous == char { depth -= 1 }
            if char == "=" && depth <= 0 {
                let key = part[..<index].trimmingCharacters(in: .whitespacesAndNewlines)
                return key.isEmpty || key.contains("\n") ? nil : index
            }
            previous = char
        }
        return nil
    }
}
