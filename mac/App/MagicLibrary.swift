// Group and identify selected music without reading or changing audio files.
import Foundation
import CryptoKit

struct MagicLibraryTrack: Sendable {
    let id: String
    let fileName: String
    let title: String
    let artist: String
    let album: String
    let albumArtist: String
    let disc: Int
    let track: Int
    let folderName: String
}

struct MagicLibraryGroup: Identifiable, Sendable {
    let id: String
    let tracks: [MagicLibraryTrack]
    let query: AlbumLookupQuery
    let summary: String
}

struct MagicLibraryOutcome: Sendable {
    let group: MagicLibraryGroup
    let release: AlbumLookupRelease?
    let details: AlbumLookupDetails?
    let safeToApply: Bool
    let reason: String
    let warnings: [String]
    let trackMatches: [String: AlbumLookupTrack]
}

struct MagicLibraryEditionCheck {
    let requiresReview: Bool
    let preserveYear: Bool
    let preserveCover: Bool
    let warnings: [String]
}

actor MagicLibraryService {
    private let lookup: AlbumLookupService
    init(lookup: AlbumLookupService = AlbumLookupService()) { self.lookup = lookup }

    nonisolated static func group(_ tracks: [MagicLibraryTrack]) -> [MagicLibraryGroup] {
        var seen = Set<String>()
        let unique = tracks.filter { !$0.id.isEmpty && seen.insert($0.id).inserted }
        // Keep a real album folder intact even when its tags disagree. Loose
        // generic folders use meaningful logical tags; unknown loose files stay
        // separate until identification supplies stronger evidence.
        var buckets: [String: [MagicLibraryTrack]] = [:]
        for track in unique {
            let directory = albumDirectory(track)
            let folder = baseFolder(track)
            let key: String
            if generic(folder) {
                if !generic(track.album) {
                    let artist = generic(track.albumArtist) ? track.artist : track.albumArtist
                    key = "tags:" + AlbumLookupMatching.normalized(track.album) + "|" + AlbumLookupMatching.normalized(artist)
                } else { key = "orphan:" + track.id }
            } else { key = "folder:" + directory }
            buckets[key, default: []].append(track)
        }
        var merged: [String: [MagicLibraryTrack]] = [:]
        for (bucket, rows) in buckets {
            let folder = baseFolder(rows[0])
            let folderQuery = AlbumLookupQuery(folderName: folder).inferred
            let artist = common(rows.map { generic($0.albumArtist) ? $0.artist : $0.albumArtist })
            let meaningfulArtist = folderQuery.artist.isEmpty ? artist : folderQuery.artist
            let key: String
            if !generic(folderQuery.album), !generic(meaningfulArtist) {
                key = "album:" + AlbumLookupMatching.normalized(folderQuery.album) + "|" + AlbumLookupMatching.normalized(meaningfulArtist)
            } else { key = bucket }
            merged[key, default: []] += rows
        }
        return merged.values.map { rows in
            let sorted = rows.sorted { $0.id.localizedStandardCompare($1.id) == .orderedAscending }
            let folder = baseFolder(sorted[0])
            let album = common(sorted.map(\.album))
            let artist = common(sorted.map { generic($0.albumArtist) ? $0.artist : $0.albumArtist })
            let query = AlbumLookupQuery(artist: artist, album: album, folderName: folder, tracks: sorted.map {
                AlbumLookupLocalTrack(fileName: $0.fileName, title: $0.title, disc: $0.disc, track: $0.track)
            })
            let digest = SHA256.hash(data: Data(sorted.map(\.id).joined(separator: "\u{0}").utf8)).map { String(format: "%02x", $0) }.joined()
            return MagicLibraryGroup(id: digest, tracks: sorted, query: query, summary: query.summary)
        }.sorted { $0.summary.localizedStandardCompare($1.summary) == .orderedAscending }
    }

    func identify(group: MagicLibraryGroup) async throws -> MagicLibraryOutcome {
        try Task.checkCancellation()
        let found = try await lookup.search(query: group.query)
        guard !found.releases.isEmpty else {
            return MagicLibraryOutcome(group: group, release: nil, details: nil, safeToApply: false,
                                       reason: "No matching album was found. Refine the search manually.", warnings: found.warnings, trackMatches: [:])
        }
        var checked: [(AlbumLookupRelease, AlbumLookupDetails, [String: AlbumLookupTrack])] = []
        var warnings = found.warnings
        // Partial selections need track evidence, so inspect a few alternatives
        // rather than assuming the first matching album name is the right one.
        for release in found.releases.prefix(3) {
            try Task.checkCancellation()
            do {
                let details = try await lookup.metadataDetails(for: release)
                let ranked = AlbumLookupMatching.rank(release, query: group.query.inferred, tracks: details.tracks)
                checked.append((ranked, details, Self.match(group.tracks, to: details.tracks)))
            } catch is CancellationError { throw CancellationError() }
            catch { warnings.append(error.localizedDescription) }
        }
        try Task.checkCancellation()
        let folderYear = Self.explicitFolderYear(group)
        checked.sort {
            if $0.2.count != $1.2.count { return $0.2.count > $1.2.count }
            let firstScore = $0.0.matchScore ?? 0, secondScore = $1.0.matchScore ?? 0
            if abs(firstScore - secondScore) <= 6, let folderYear {
                let firstYear = $0.1.year == folderYear, secondYear = $1.1.year == folderYear
                if firstYear != secondYear { return firstYear }
            }
            return firstScore > secondScore
        }
        guard let best = checked.first else {
            return MagicLibraryOutcome(group: group, release: found.releases.first, details: nil, safeToApply: false,
                                       reason: "The track list could not be checked. Review this album manually.", warnings: warnings, trackMatches: [:])
        }
        let release = best.0, matches = best.2
        var details = best.1
        let editions = Self.compareEditions(group: group, chosen: release, details: details, matches: matches, alternatives: Array(checked.dropFirst()))
        warnings += editions.warnings
        if !editions.preserveCover {
            do { details = try await lookup.details(for: release) }
            catch is CancellationError { throw CancellationError() }
            catch { warnings.append("The cover could not be downloaded. " + error.localizedDescription) }
        }
        var preparedCover = editions.preserveCover ? nil : details.cover
        var coverWarning = details.coverWarning
        if editions.preserveCover { coverWarning = "The edition is uncertain, so your current cover will be kept." }
        if let cover = preparedCover {
            do { preparedCover = try Self.mappedArtwork(cover) }
            catch {
                preparedCover = nil
                coverWarning = "The cover could not be prepared for this batch. Your current cover will be kept. " + error.localizedDescription
                warnings.append(coverWarning!)
            }
        }
        details = AlbumLookupDetails(title: details.title, artist: details.artist, year: editions.preserveYear ? nil : details.year,
                                     genre: details.genre, tracks: details.tracks, cover: preparedCover, coverWarning: coverWarning,
                                     coverSource: preparedCover == nil ? nil : details.coverSource)
        let query = group.query.inferred
        let folder = AlbumLookupQuery(folderName: group.query.folderName).inferred
        let albumEvidence = [query.album, folder.album].filter { !Self.generic($0) }.map { AlbumLookupMatching.similarity($0, details.title) }.max() ?? 0
        let artistEvidence = [query.artist, folder.artist].filter { !Self.generic($0) }.map { AlbumLookupMatching.similarity($0, details.artist) }.max() ?? 0
        let complete = matches.count == group.tracks.count && !matches.isEmpty
        let distinctNames = Set(group.tracks.flatMap { Self.names($0) }).count
        let multipleEvidence = group.tracks.count >= 2 && distinctNames >= 2
        let singleEvidence = group.tracks.count == 1 && (Self.names(group.tracks[0]).map(\.count).max() ?? 0) >= 10 && albumEvidence >= 0.8 && artistEvidence >= 0.8
        let contextual = albumEvidence >= 0.75 || artistEvidence >= 0.8
        // The service's count-based score can be lower for a selected subset.
        // Several unique, exact title matches plus independent album/artist
        // context provide stronger evidence than the incomplete count itself.
        let score = release.matchScore ?? 0
        let strongPartial = group.tracks.count < details.tracks.count && complete && multipleEvidence && contextual && score >= 65
        let safe = !editions.requiresReview && complete && contextual && (multipleEvidence || singleEvidence) && (score >= 85 || strongPartial)
        let reason: String
        if editions.requiresReview {
            reason = "Several near-matching editions disagree on the album or selected track numbering. Review this group manually."
        } else if safe {
            reason = group.tracks.count == details.tracks.count ? "Confirmed track names, album context and complete numbering agree." : "Every selected track has a unique match within this album; only the selected subset will change."
        } else if !complete {
            reason = "Matched \(matches.count) of \(group.tracks.count) selected tracks. Review the remaining matches manually."
        } else {
            reason = "Track names match, but there is not enough independent evidence to choose this edition automatically."
        }
        if let warning = details.coverWarning { warnings.append(warning) }
        return MagicLibraryOutcome(group: group, release: release, details: details, safeToApply: safe,
                                   reason: reason, warnings: Array(Set(warnings)).sorted(), trackMatches: matches)
    }

    nonisolated static func compareEditions(group: MagicLibraryGroup, chosen: AlbumLookupRelease, details: AlbumLookupDetails,
                                            matches: [String: AlbumLookupTrack],
                                            alternatives: [(AlbumLookupRelease, AlbumLookupDetails, [String: AlbumLookupTrack])]) -> MagicLibraryEditionCheck {
        var review = false, preserveYear = false, preserveCover = false
        let folderYear = explicitFolderYear(group)
        let fullNumbered = group.tracks.count == details.tracks.count && completeFilenameOrder(group.tracks, source: details.tracks)
        func identity(_ text: String) -> String { AlbumLookupMatching.normalized(text).replacingOccurrences(of: " ", with: "") }
        for (release, alternate, mapping) in alternatives {
            guard mapping.count == group.tracks.count, mapping.count == matches.count,
                  (chosen.matchScore ?? 0) - (release.matchScore ?? 0) <= 6 else { continue }
            // A complete numbered rip independently identifies its track count;
            // an otherwise similar bonus-disc release is not an equal candidate.
            if fullNumbered && alternate.tracks.count != details.tracks.count { continue }
            // An explicit folder edition year resolves an otherwise equal reissue.
            if let folderYear, details.year == folderYear, let other = alternate.year, other != folderYear { continue }
            let titleDiffers = identity(details.title) != identity(alternate.title)
            let artistDiffers = !details.artist.isEmpty && !alternate.artist.isEmpty && identity(details.artist) != identity(alternate.artist)
            let numberingDiffers = matches.contains { id, track in
                guard let other = mapping[id] else { return true }
                return track.disc != other.disc || track.track != other.track || track.trackTotal != other.trackTotal ||
                    identity(track.title) != identity(other.title) ||
                    (!track.artist.isEmpty && !other.artist.isEmpty && identity(track.artist) != identity(other.artist))
            }
            if titleDiffers || artistDiffers || numberingDiffers {
                review = true
                preserveCover = true
            } else if let year = details.year, let other = alternate.year, year != other {
                // Common tag fixes remain safe, but neither the date nor the
                // edition-specific artwork can be inferred from identical songs.
                preserveYear = true
                preserveCover = true
            }
        }
        var warnings: [String] = []
        if review { warnings.append("Plausible editions disagree on album identity or selected-track numbering; no automatic changes were made to this group.") }
        if preserveYear { warnings.append("Matching editions have different release years. Common tag fixes can be applied, but the existing year and cover are preserved.") }
        return MagicLibraryEditionCheck(requiresReview: review, preserveYear: preserveYear, preserveCover: preserveCover, warnings: warnings)
    }

    // A mapped, unlinked file preserves image quality while allowing the OS to
    // reclaim cover pages between previews. The music files are never involved.
    nonisolated static func mappedArtwork(_ data: Data) throws -> Data {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("betterpod-magic-art-" + UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
        defer { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appendingPathComponent("cover")
        try data.write(to: file, options: .atomic)
        return try Data(contentsOf: file, options: .alwaysMapped)
    }

    nonisolated private static func explicitFolderYear(_ group: MagicLibraryGroup) -> Int? {
        let inputs = [group.query.folderName] + group.tracks.flatMap { [$0.folderName, URL(fileURLWithPath: albumDirectory($0)).lastPathComponent] }
        let regex = try! NSRegularExpression(pattern: "[\\[(]((?:19|20)\\d{2})[\\])]")
        var years = Set<Int>()
        for text in inputs {
            for match in regex.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
                if let range = Range(match.range(at: 1), in: text), let year = Int(text[range]) { years.insert(year) }
            }
        }
        return years.count == 1 ? years.first : nil
    }

    nonisolated private static func completeFilenameOrder(_ tracks: [MagicLibraryTrack], source: [AlbumLookupTrack]) -> Bool {
        let local = tracks.compactMap(filenamePosition).map { "\($0.disc):\($0.track)" }
        let expected = source.map { "\($0.disc):\($0.track)" }
        return local.count == tracks.count && Set(local).count == tracks.count && Set(local) == Set(expected)
    }

    nonisolated static func match(_ local: [MagicLibraryTrack], to source: [AlbumLookupTrack]) -> [String: AlbumLookupTrack] {
        var proposals: [String: Int] = [:]
        var uses: [Int: Int] = [:]
        let fileArtist = AlbumLookupQuery(tracks: local.map { AlbumLookupLocalTrack(fileName: $0.fileName, title: "") }).inferred.artist
        for track in local {
            let wanted = names(track, fileArtist: fileArtist)
            guard !wanted.isEmpty else { continue }
            let candidates = source.indices.filter { wanted.contains(AlbumLookupMatching.normalized(source[$0].title)) }
            if candidates.count == 1 {
                proposals[track.id] = candidates[0]
                uses[candidates[0], default: 0] += 1
            }
        }
        var matches: [String: AlbumLookupTrack] = [:]
        for (id, index) in proposals where uses[index] == 1 { matches[id] = source[index] }
        // Complete a full rip only when filename numbering is complete and
        // already-confirmed independent titles establish that the order agrees.
        // Numbers alone never authorize partial or entirely unnamed selections.
        if local.count == source.count, matches.count >= max(2, (local.count + 1) / 2) {
            let positions = local.map(filenamePosition)
            let sourceKeys = source.map { "\($0.disc):\($0.track)" }
            let localKeys = positions.compactMap { $0.map { "\($0.disc):\($0.track)" } }
            let contiguous = Dictionary(grouping: source, by: \.disc).values.allSatisfy { disc in
                Set(disc.map(\.track)) == Set(1...disc.count)
            }
            if contiguous && localKeys.count == local.count && Set(localKeys).count == local.count && Set(localKeys) == Set(sourceKeys) {
                let confirmedOrder = zip(local, positions).allSatisfy { track, position in
                    guard let match = matches[track.id], let position else { return true }
                    return match.disc == position.disc && match.track == position.track
                }
                if confirmedOrder {
                    let byPosition = Dictionary(uniqueKeysWithValues: zip(sourceKeys, source))
                    for (track, key) in zip(local, localKeys) where matches[track.id] == nil { matches[track.id] = byPosition[key] }
                }
            }
        }
        return matches
    }

    nonisolated private static func names(_ track: MagicLibraryTrack, fileArtist: String = "") -> Set<String> {
        let artist = generic(track.albumArtist) ? track.artist : track.albumArtist
        let row = AlbumLookupLocalTrack(fileName: track.fileName, title: track.title, disc: track.disc, track: track.track)
        let file = AlbumLookupLocalTrack(fileName: track.fileName, title: "")
        let folderArtist = AlbumLookupQuery(folderName: baseFolder(track)).inferred.artist
        return Set([AlbumLookupMatching.localTitle(row, artist: artist), AlbumLookupMatching.localTitle(file, artist: artist),
                    AlbumLookupMatching.localTitle(file, artist: fileArtist), AlbumLookupMatching.localTitle(file, artist: folderArtist)]
            .map(AlbumLookupMatching.normalized).filter { !generic($0) && !$0.isEmpty && $0.range(of: "^(?:track|audio|song)? ?[0-9]+$", options: .regularExpression) == nil })
    }
    nonisolated private static func generic(_ text: String) -> Bool {
        let value = AlbumLookupMatching.normalized(text)
        return AlbumLookupMatching.placeholder(text) || ["songs", "tracks", "loose", "unsorted", "new folder", "untitled album"].contains(value) || value.range(of: "^(music|downloads?|songs?|tracks?|loose|unsorted|unknown) ?[0-9]+$", options: .regularExpression) != nil
    }
    nonisolated private static func common(_ values: [String]) -> String {
        let meaningful = values.filter { !generic($0) }
        guard !meaningful.isEmpty else { return "" }
        let groups = Dictionary(grouping: meaningful, by: AlbumLookupMatching.normalized)
        guard let best = groups.values.max(by: { $0.count < $1.count }), best.count * 2 >= values.count else { return "" }
        return best[0]
    }
    nonisolated private static func filenamePosition(_ track: MagicLibraryTrack) -> (disc: Int, track: Int)? {
        guard var position = AlbumLookupMatching.filenamePosition(track.fileName) else { return nil }
        let explicitDisc = track.fileName.range(of: "(?i)^\\s*(?:(?:cd|disc)\\s*)?\\d{1,2}[-._]\\d{1,3}(?:\\s|[-._])", options: .regularExpression) != nil
        let directory = URL(fileURLWithPath: track.id).deletingLastPathComponent().lastPathComponent
        if !explicitDisc && isDisc(directory), let disc = Int(directory.filter(\.isNumber)), disc > 0 { position.disc = disc }
        return position
    }
    nonisolated private static func albumDirectory(_ track: MagicLibraryTrack) -> String {
        var directory = URL(fileURLWithPath: track.id).deletingLastPathComponent()
        if isDisc(directory.lastPathComponent) { directory.deleteLastPathComponent() }
        return directory.path
    }
    nonisolated private static func baseFolder(_ track: MagicLibraryTrack) -> String {
        var folder = track.folderName
        if folder.isEmpty || isDisc(folder) {
            folder = URL(fileURLWithPath: albumDirectory(track)).lastPathComponent
        }
        return AlbumLookupMatching.cleanFolder(folder)
    }
    nonisolated private static func isDisc(_ text: String) -> Bool {
        text.range(of: "(?i)^(cd|disc|disk)[ _.-]*[0-9]+$", options: .regularExpression) != nil
    }
}
