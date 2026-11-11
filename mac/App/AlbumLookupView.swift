import SwiftUI

/// Find Album: searches every source for one album, previews the best match
/// and applies its cover, album details and track names to the draft. A
/// selection of several albums is handled one album at a time.
struct AlbumLookupView: View {
    @ObservedObject var model: LibraryModel
    let targetIDs: Set<String>
    var step = 1
    var steps = 1
    @State private var artist = ""
    @State private var album = ""
    @State private var releases: [AlbumLookupRelease] = []
    @State private var selectedID: String?
    @State private var provider: AlbumLookupProvider?
    @State private var details: AlbumLookupDetails?
    @State private var searching = false
    @State private var loadingDetails = false
    @State private var searched = false
    @State private var error: String?
    @State private var warnings: [String] = []
    @State private var includeMetadata = true
    @State private var includeTracks = true
    @State private var includeArtwork = true
    @State private var request: Task<Void, Never>?
    @State private var requestID = UUID()
    @State private var service = AlbumLookupService()

    private var targets: [LibraryDraft] { model.orderedTargets(targetIDs) }
    private var visibleReleases: [AlbumLookupRelease] {
        provider.map { source in releases.filter { $0.provider == source } } ?? releases
    }
    private var chosen: AlbumLookupRelease? { releases.first { $0.id == selectedID } }
    private var sourceTracks: [AlbumLookupTrack] {
        (details?.tracks ?? []).sorted { $0.disc == $1.disc ? $0.track < $1.track : $0.disc < $1.disc }
    }
    private var canMapTracks: Bool {
        !sourceTracks.isEmpty && sourceTracks.count == targets.count &&
        sourceTracks.allSatisfy { $0.track > 0 && $0.disc > 0 } &&
        Set(sourceTracks.map { "\($0.disc)-\($0.track)" }).count == sourceTracks.count
    }
    private var canApply: Bool {
        details != nil && !searching && !loadingDetails && !model.busy && !model.remoteNeedsReload && targets.contains(where: { $0.original.writable }) &&
        (includeMetadata || (includeArtwork && details?.cover != nil))
    }
    private var hasNext: Bool { step < steps }
    private var query: AlbumLookupQuery {
        AlbumLookupQuery(artist: artist, album: album, folderName: folderName,
                         tracks: targets.map { AlbumLookupLocalTrack(fileName: $0.original.fileName, title: $0[.title], disc: Int($0[.disc]) ?? 0, track: Int($0[.track]) ?? 0) })
    }
    private var folderName: String {
        let folders = Set(targets.map { URL(fileURLWithPath: $0.id).deletingLastPathComponent().path })
        if folders.count == 1, let folder = folders.first {
            let url = URL(fileURLWithPath: folder)
            if url.lastPathComponent.range(of: #"(?i)^(cd|disc|disk)[ ._-]*\d+$"#, options: .regularExpression) != nil {
                return url.deletingLastPathComponent().lastPathComponent
            }
            return url.lastPathComponent
        }
        return model.lookupFolderName
    }

    var body: some View {
        VStack(spacing: 0) {
            PodSheetHeader(title: "Find Album",
                           subtitle: (steps > 1 ? "Album \(step) of \(steps) · " : "") + "\(targets.count) \(targets.count == 1 ? "track" : "tracks")" + (folderName.isEmpty ? "" : " · \(folderName)")) {
                if searching || loadingDetails {
                    ProgressView().controlSize(.small)
                        .accessibilityLabel(searching ? "Searching" : "Loading the album")
                }
            }
            searchBar
            HStack(spacing: 0) {
                candidates.frame(width: 290)
                Pod.hairline.frame(width: 1)
                preview.frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Color(nsColor: .textBackgroundColor))
            }
            if let error {
                PodBanner(text: error, isError: true)
            }
            if !warnings.isEmpty {
                DisclosureGroup("Some sources did not answer") {
                    ForEach(Array(warnings.enumerated()), id: \.offset) { _, warning in
                        Text(warning).font(.caption).foregroundStyle(Pod.secondaryText)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .font(.caption).padding(.horizontal, 16).padding(.vertical, 7)
                .background(Pod.strip)
            }
            PodSheetFooter {
                Text(model.isRemote ? "betterPod changes only when you save." : "Your files change only when you save.")
                    .font(.caption).foregroundStyle(Pod.secondaryText)
                Spacer()
                Button("Cancel") { request?.cancel(); model.showingLookup = false }
                    .keyboardShortcut(.cancelAction)
                if steps > 1 {
                    Button(hasNext ? "Skip Album" : "Skip") { request?.cancel(); model.nextLookup() }
                }
                Button(hasNext ? "Apply and Next" : "Apply") {
                    guard let details else { return }
                    model.apply(details, to: targetIDs, includeTracks: includeTracks && canMapTracks,
                                includeArtwork: includeArtwork, includeMetadata: includeMetadata)
                    request?.cancel()
                    model.nextLookup()
                }
                .buttonStyle(PodButtonStyle(kind: .primary))
                .disabled(!canApply)
                .keyboardShortcut(.defaultAction)
            }
            .buttonStyle(PodButtonStyle())
        }
        .frame(minWidth: 880, idealWidth: 980, minHeight: 600, idealHeight: 720)
        .background(Pod.panel)
        .onAppear {
            artist = majority(targets.map { $0.effectiveAlbumArtist.isEmpty ? $0[.artist] : $0.effectiveAlbumArtist })
            album = majority(targets.map(\.effectiveAlbum))
            let inferred = query.inferred
            artist = inferred.artist
            album = inferred.album
            search()
        }
        .onChange(of: selectedID) { id in
            if let id, let release = releases.first(where: { $0.id == id }) { load(release) }
        }
        .onChange(of: provider) { _ in
            guard !searching else { return }
            if !visibleReleases.contains(where: { $0.id == selectedID }) {
                selectedID = visibleReleases.first?.id
                if selectedID == nil { request?.cancel(); details = nil; loadingDetails = false }
            }
        }
        .onDisappear { request?.cancel() }
    }

    private var searchBar: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                TextField("Artist", text: $artist).textFieldStyle(.roundedBorder)
                    .accessibilityLabel("Search Artist").onSubmit { search() }
                TextField("Album, or a Discogs link", text: $album).textFieldStyle(.roundedBorder)
                    .accessibilityLabel("Search Album").onSubmit { search() }
                Button { search() } label: { Label("Search", systemImage: "magnifyingglass") }
                    .buttonStyle(PodButtonStyle())
                    .disabled(artist.trimmingCharacters(in: .whitespaces).isEmpty && album.trimmingCharacters(in: .whitespaces).isEmpty && targets.isEmpty)
            }
            Text("Searches MusicBrainz, Discogs, iTunes, Deezer and Wikipedia with the folder, tags and track names. Paste a Discogs link to use that exact edition.")
                .font(.caption).foregroundStyle(Pod.secondaryText)
        }
        .padding(.horizontal, 16).padding(.vertical, 10)
        .background(Pod.strip)
        .overlay(alignment: .bottom) { Pod.hairline.frame(height: 1) }
    }

    private var candidates: some View {
        VStack(spacing: 0) {
            HStack {
                PodSectionTitle("Matches")
                Spacer()
                Picker("Source", selection: $provider) {
                    Text("All Sources").tag(Optional<AlbumLookupProvider>.none)
                    ForEach(AlbumLookupProvider.allCases, id: \.self) { source in
                        Text(source.name).tag(Optional(source))
                    }
                }
                .labelsHidden().fixedSize().controlSize(.small)
            }
            .padding(.horizontal, 12).padding(.vertical, 8)
            .overlay(alignment: .bottom) { Pod.hairline.frame(height: 1) }
            if visibleReleases.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: "magnifyingglass").font(.title2).foregroundStyle(Pod.secondaryText)
                    Text(searching ? "Finding Your Album…" : (searched ? "No Matching Albums" : "Ready to Search"))
                        .font(.headline)
                    Text(searching ? "Comparing editions and track lists." : "Edit the album or artist, paste a Discogs link, or choose another source.")
                        .font(.callout).foregroundStyle(Pod.secondaryText).multilineTextAlignment(.center)
                }
                .padding(20).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(visibleReleases) { release in
                                MatchRow(release: release, best: release.id == releases.first?.id, selected: release.id == selectedID)
                                    .id(release.id)
                                    .onTapGesture { selectedID = release.id }
                            }
                        }
                    }
                    .focusable()
                    .onMoveCommand { direction in
                        guard let index = visibleReleases.firstIndex(where: { $0.id == selectedID }) else {
                            selectedID = visibleReleases.first?.id
                            return
                        }
                        let next = direction == .up ? index - 1 : (direction == .down ? index + 1 : index)
                        guard visibleReleases.indices.contains(next) else { return }
                        selectedID = visibleReleases[next].id
                        proxy.scrollTo(visibleReleases[next].id)
                    }
                    .accessibilityLabel("Matches")
                }
            }
        }
        .background(Color(nsColor: .textBackgroundColor))
    }

    @ViewBuilder private var preview: some View {
        if let details, let chosen {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    HStack(alignment: .top, spacing: 16) {
                        LibraryCover(data: details.cover, cornerRadius: 4)
                            .frame(width: 150, height: 150)
                            .shadow(color: .black.opacity(0.28), radius: 4, y: 2)
                        VStack(alignment: .leading, spacing: 5) {
                            Text(details.title).font(.system(size: 17, weight: .bold))
                            Text(details.artist).font(.callout).foregroundStyle(Pod.secondaryText)
                            Text([details.year.map(String.init), details.genre, "\(details.tracks.count) tracks"].compactMap { $0 }.joined(separator: " · "))
                                .font(.callout).foregroundStyle(Pod.secondaryText)
                            Link("View on \(chosen.provider.name)", destination: chosen.sourceURL).font(.caption)
                            if let summary = chosen.matchSummary { Text(summary).font(.caption).foregroundStyle(Pod.secondaryText) }
                            if let source = details.coverSource, details.cover != nil {
                                Text("Cover from \(source)").font(.caption).foregroundStyle(Pod.secondaryText)
                            }
                        }
                    }
                    PodCard(title: "Apply") {
                        Toggle("Cover", isOn: $includeArtwork).disabled(details.cover == nil)
                        Toggle("Album details: album, album artist, year and genre", isOn: $includeMetadata)
                        Toggle("Track names and numbers", isOn: $includeTracks)
                            .disabled(!includeMetadata || !canMapTracks)
                        if details.cover == nil {
                            Text(details.coverWarning ?? "No cover is available for this result. Your current cover will be kept.")
                                .font(.caption).foregroundStyle(Pod.secondaryText)
                        }
                        if !canMapTracks {
                            Label("This edition has \(sourceTracks.count) tracks and you selected \(targets.count). Track names stay as they are.", systemImage: "info.circle")
                                .font(.caption).foregroundStyle(Pod.secondaryText)
                        }
                    }
                    .toggleStyle(.checkbox)
                    if includeMetadata {
                        PodCard(title: "Album") {
                            albumChange("Album", current: majority(targets.map(\.effectiveAlbum)), proposed: details.title)
                            albumChange("Album artist", current: majority(targets.map(\.effectiveAlbumArtist)), proposed: details.artist)
                            if let year = details.year { albumChange("Year", current: majority(targets.map { $0[.year] }), proposed: String(year)) }
                            if let genre = details.genre { albumChange("Genre", current: majority(targets.map { $0[.genre] }), proposed: genre) }
                        }
                    }
                    if includeMetadata && includeTracks && canMapTracks {
                        PodCard(title: "Tracks") {
                            VStack(spacing: 0) {
                                ForEach(Array(targets.enumerated()), id: \.element.id) { index, local in
                                    trackPair(local, proposed: sourceTracks[index], striped: index % 2 == 1)
                                }
                            }
                            .padding(.horizontal, -14).padding(.bottom, -14)
                        }
                    }
                }
                .padding(18)
            }
        } else {
            VStack(spacing: 12) {
                Image(systemName: "opticaldisc").font(.system(size: 40, weight: .light)).foregroundStyle(Pod.secondaryText.opacity(0.7))
                Text(loadingDetails ? "Loading the Album…" : (searching ? "Identifying Your Album…" : "Choose a Match"))
                    .font(.headline)
                Text("The best match is chosen for you. Compare other editions before applying.")
                    .font(.callout).foregroundStyle(Pod.secondaryText).multilineTextAlignment(.center).frame(maxWidth: 320)
            }
            .padding(30).frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func albumChange(_ label: String, current: String, proposed: String) -> some View {
        let changes = !proposed.isEmpty && current != proposed
        return HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(label).font(.caption).foregroundStyle(Pod.secondaryText).frame(width: 86, alignment: .leading)
            VStack(alignment: .leading, spacing: 2) {
                Text(proposed.isEmpty ? "Keep current value" : proposed).font(.callout.weight(changes ? .semibold : .regular))
                if changes {
                    Text(current.isEmpty ? "Empty or several values" : current).font(.caption).foregroundStyle(Pod.secondaryText).strikethrough()
                }
            }
            Spacer()
            if changes { Image(systemName: "pencil.circle.fill").foregroundStyle(Pod.edited).accessibilityLabel("Will change") }
        }
    }

    private func trackPair(_ local: LibraryDraft, proposed: AlbumLookupTrack, striped: Bool) -> some View {
        let changes = local[.title] != proposed.title
        return HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text("\(proposed.disc).\(proposed.track)").font(.caption.monospacedDigit()).foregroundStyle(Pod.secondaryText).frame(width: 34, alignment: .trailing)
            VStack(alignment: .leading, spacing: 2) {
                Text(proposed.title).font(.callout.weight(changes ? .semibold : .regular))
                Text(local.original.fileName).font(.caption).foregroundStyle(Pod.secondaryText).lineLimit(1).truncationMode(.middle)
            }
            Spacer()
            if changes { Image(systemName: "pencil.circle.fill").foregroundStyle(Pod.edited).help(local[.title].isEmpty ? "New title" : "Previously: \(local[.title])") }
        }
        .padding(.horizontal, 14).padding(.vertical, 6)
        .background(striped ? Pod.strip.opacity(0.6) : Color.clear)
    }

    private func majority(_ values: [String]) -> String {
        let values = values.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
        let counts = Dictionary(grouping: values, by: { $0 }).mapValues(\.count)
        return counts.keys.sorted {
            if counts[$0] != counts[$1] { return counts[$0, default: 0] > counts[$1, default: 0] }
            return $0.localizedStandardCompare($1) == .orderedAscending
        }.first ?? ""
    }

    private func search() {
        request?.cancel()
        let id = UUID()
        requestID = id
        searching = true
        loadingDetails = false
        error = nil
        warnings = []
        details = nil
        selectedID = nil
        releases = []
        let searchQuery = query
        request = Task { @MainActor in
            do {
                let result = try await service.search(query: searchQuery)
                try Task.checkCancellation()
                guard requestID == id else { return }
                releases = result.releases
                warnings = result.warnings
                searched = true
                searching = false
                selectedID = visibleReleases.first?.id
            } catch is CancellationError {
                if requestID == id { searching = false }
            } catch {
                guard requestID == id else { return }
                searching = false
                searched = true
                self.error = error.localizedDescription
            }
        }
    }

    private func load(_ release: AlbumLookupRelease) {
        request?.cancel()
        let id = UUID()
        requestID = id
        loadingDetails = true
        searching = false
        details = nil
        error = nil
        request = Task { @MainActor in
            do {
                let result = try await service.details(for: release)
                try Task.checkCancellation()
                guard requestID == id else { return }
                details = result
                loadingDetails = false
                includeTracks = canMapTracks
            } catch is CancellationError {
                if requestID == id { loadingDetails = false }
            } catch {
                guard requestID == id else { return }
                loadingDetails = false
                self.error = error.localizedDescription
            }
        }
    }
}

/// A search result; the selected one wears the console's blue cursor.
private struct MatchRow: View {
    let release: AlbumLookupRelease
    let best: Bool
    let selected: Bool
    @Environment(\.controlActiveState) private var activeState

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline) {
                Text(release.title).font(.system(size: 13, weight: .semibold)).lineLimit(2)
                Spacer(minLength: 4)
                if let score = release.matchScore {
                    Text("\(score)")
                        .font(.system(size: 10.5, weight: .bold).monospacedDigit())
                        .padding(.horizontal, 5).padding(.vertical, 1)
                        .background(Capsule().fill(selected ? Color.white.opacity(0.25) : (best ? Pod.saved.opacity(0.18) : Pod.strip)))
                        .help(best ? "Best match, \(score) out of 100" : "Match, \(score) out of 100")
                }
            }
            Text(release.artist).font(.callout).lineLimit(1)
                .foregroundStyle(selected ? Color.white.opacity(0.9) : Pod.secondaryText)
            Text([release.sourceName, release.year.map(String.init), release.trackCount.map { "\($0) tracks" }]
                .compactMap { $0 }.joined(separator: " · "))
                .font(.caption).lineLimit(1)
                .foregroundStyle(selected ? Color.white.opacity(0.85) : Pod.secondaryText)
        }
        .foregroundStyle(selected ? Color.white : Color.primary)
        .padding(.horizontal, 12).padding(.vertical, 7)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background {
            if selected { PodSelection(active: activeState != .inactive) }
        }
        .overlay(alignment: .bottom) { if !selected { Pod.hairline.opacity(0.5).frame(height: 1) } }
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? [.isSelected, .isButton] : .isButton)
    }
}
