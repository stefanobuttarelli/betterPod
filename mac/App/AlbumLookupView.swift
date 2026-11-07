import SwiftUI

struct AlbumLookupView: View {
    @ObservedObject var model: LibraryModel
    let targetIDs: Set<String>
    var artworkOnly: Bool? = nil
    var onApply: (() -> Void)? = nil
    @Environment(\.dismiss) private var dismiss
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
    private var isArtworkOnly: Bool { artworkOnly ?? model.lookupArtworkOnly }
    private var applyTitle: String {
        if includeMetadata && includeArtwork && details?.cover != nil { return "Apply Metadata and Artwork" }
        return includeMetadata ? "Apply Metadata" : "Apply Artwork"
    }
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
            searchHeader
            Divider()
            HSplitView {
                candidates.frame(minWidth: 235, idealWidth: 270, maxWidth: 320)
                preview.frame(minWidth: 470, maxWidth: .infinity, maxHeight: .infinity)
            }
            if let error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .font(.callout).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(14)
            }
            if !warnings.isEmpty {
                DisclosureGroup("Service Availability") {
                    ForEach(Array(warnings.enumerated()), id: \.offset) { _, warning in
                        Text(warning).font(.caption).foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .font(.caption).padding(.horizontal, 18).padding(.vertical, 10)
            }
            Divider()
            HStack(spacing: 12) {
                Text("\(targets.count) Selected Tracks · \(model.isRemote ? "betterPod Changes Only When You Save" : "Files Change Only When You Save")")
                    .font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button("Cancel") { close() }.keyboardShortcut(.cancelAction)
                Button(applyTitle) {
                    guard let details else { return }
                    model.apply(details, to: targetIDs, includeTracks: includeTracks && canMapTracks,
                                includeArtwork: includeArtwork, includeMetadata: includeMetadata)
                    onApply?()
                    dismiss()
                }
                .buttonStyle(AppleProminentButtonStyle())
                .disabled(!canApply)
                .keyboardShortcut(.defaultAction)
            }.padding(18)
        }
        .frame(minWidth: 850, idealWidth: 960, minHeight: 580, idealHeight: 700)
        .background(Color(nsColor: .windowBackgroundColor))
        .onAppear {
            includeMetadata = !isArtworkOnly
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

    private var searchHeader: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                VStack(alignment: .leading, spacing: 5) {
                    Text(isArtworkOnly ? "Find Cover Artwork" : "Find Album Details").font(.title2.weight(.semibold))
                    Text("Matching folder, track names and numbering across MusicBrainz, iTunes and Deezer.")
                        .font(.callout).foregroundStyle(.secondary)
                }
                Spacer()
                if searching || loadingDetails {
                    ProgressView().controlSize(.small)
                        .accessibilityLabel(searching ? "Identifying Album" : "Loading Album Preview")
                }
            }
            HStack(spacing: 10) {
                TextField("Artist", text: $artist).textFieldStyle(.roundedBorder)
                    .accessibilityLabel("Search Artist").onSubmit { search() }
                TextField("Album", text: $album).textFieldStyle(.roundedBorder)
                    .accessibilityLabel("Search Album").onSubmit { search() }
                Button(searching ? "Search Again" : "Search") { search() }
                    .buttonStyle(AppleToolbarButtonStyle())
                    .disabled(artist.trimmingCharacters(in: .whitespaces).isEmpty && album.trimmingCharacters(in: .whitespaces).isEmpty && targets.isEmpty)
            }
            HStack(spacing: 6) {
                Image(systemName: "folder").foregroundStyle(.secondary)
                Text(folderName.isEmpty ? "Using Track Names and Numbers" : folderName)
                    .font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                Spacer()
                if searching { Text("Comparing Album Editions…").font(.caption).foregroundStyle(.secondary) }
            }
        }.padding(20)
    }

    private var candidates: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Matches").font(.headline)
                Spacer()
                Picker("Source", selection: $provider) {
                    Text("All Services").tag(Optional<AlbumLookupProvider>.none)
                    ForEach(AlbumLookupProvider.allCases, id: \.self) { source in
                        Text(source.name).tag(Optional(source))
                    }
                }.labelsHidden().fixedSize()
            }.padding(14)
            if visibleReleases.isEmpty {
                VStack(spacing: 10) {
                    Image(systemName: searching ? "sparkle.magnifyingglass" : "magnifyingglass").font(.title2).foregroundStyle(.secondary)
                    Text(searching ? "Finding Your Album…" : (searched ? "No Matching Albums" : "Ready to Search"))
                        .font(.headline)
                    Text(searching ? "Checking available services and comparing track lists." : "Try editing the album or artist, or choose another service.")
                        .font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
                }.padding(20).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                List(visibleReleases, selection: $selectedID) { release in
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(alignment: .firstTextBaseline) {
                            Text(release.title).fontWeight(.medium).lineLimit(2)
                            Spacer(minLength: 2)
                            if release.id == releases.first?.id {
                                Image(systemName: "star.fill").font(.caption).accessibilityLabel("Best Match")
                            }
                        }
                        Text(release.artist).font(.callout).foregroundStyle(.secondary).lineLimit(1)
                        Text([release.provider.name, release.year.map(String.init), release.trackCount.map { "\($0) Tracks" }]
                            .compactMap { $0 }.joined(separator: " · "))
                            .font(.caption).foregroundStyle(.secondary)
                        if let score = release.matchScore {
                            Text(release.id == releases.first?.id ? "Best Match · \(score)/100" : "Match · \(score)/100")
                                .font(.caption).fontWeight(.medium)
                        }
                    }.padding(.vertical, 5).tag(release.id)
                }.listStyle(.inset)
            }
        }
    }

    @ViewBuilder private var preview: some View {
        if let details, let chosen {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    HStack(alignment: .top, spacing: 18) {
                        LibraryCover(data: details.cover).frame(width: 140, height: 140)
                        VStack(alignment: .leading, spacing: 8) {
                            Text(details.title).font(.title3.weight(.semibold))
                            Text(details.artist).foregroundStyle(.secondary)
                            Text([details.year.map(String.init), "\(details.tracks.count) Tracks"].compactMap { $0 }.joined(separator: " · "))
                                .font(.callout).foregroundStyle(.secondary)
                            Link("View on \(chosen.provider.name)", destination: chosen.sourceURL).font(.caption)
                            if let summary = chosen.matchSummary { Text(summary).font(.caption).foregroundStyle(.secondary) }
                        }
                    }
                    VStack(alignment: .leading, spacing: 12) {
                        Toggle("Update Album Metadata", isOn: $includeMetadata)
                        Toggle("Update Track Titles, Artists and Numbering", isOn: $includeTracks)
                            .disabled(!includeMetadata || !canMapTracks)
                        Toggle("Replace Cover Artwork", isOn: $includeArtwork).disabled(details.cover == nil)
                    }.toggleStyle(.checkbox)
                    if let warning = details.coverWarning { Text(warning).font(.caption).foregroundStyle(.secondary) }
                    if let source = details.coverSource, details.cover != nil {
                        Text("Artwork from \(source)").font(.caption).foregroundStyle(.secondary)
                    }
                    if details.cover == nil {
                        Text("No cover is available for this result. Your current artwork will be kept.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    if includeMetadata {
                        Divider()
                        Text("Album Changes").font(.headline)
                        albumChange("Album", current: majority(targets.map(\.effectiveAlbum)), proposed: details.title)
                        albumChange("Album Artist", current: majority(targets.map(\.effectiveAlbumArtist)), proposed: details.artist)
                        if let year = details.year { albumChange("Year", current: majority(targets.map { $0[.year] }), proposed: String(year)) }
                        if let genre = details.genre { albumChange("Genre", current: majority(targets.map { $0[.genre] }), proposed: genre) }
                    }
                    if includeMetadata && includeTracks {
                        Divider()
                        Text("Track Pairings").font(.headline)
                        if canMapTracks {
                            Text("Filename numbering is used when complete, then existing disc and track numbers. Check the pairings before applying.")
                                .font(.caption).foregroundStyle(.secondary)
                            ForEach(Array(targets.enumerated()), id: \.element.id) { index, local in
                                trackPair(local, proposed: sourceTracks[index])
                            }
                        } else {
                            Label("This result has \(sourceTracks.count) tracks; you selected \(targets.count). Track fields will be kept. Album metadata and artwork can still be applied.", systemImage: "info.circle")
                                .font(.callout).foregroundStyle(.secondary)
                        }
                    }
                }.padding(22)
            }
        } else {
            VStack(spacing: 14) {
                Image(systemName: loadingDetails ? "opticaldisc" : "square.stack").font(.system(size: 40)).foregroundStyle(.tertiary)
                Text(loadingDetails ? "Loading Album Preview…" : (searching ? "Identifying Your Album…" : "Choose an Album to Preview"))
                    .font(.headline)
                Text("The best match is selected automatically. You can compare other editions before applying any changes.")
                    .font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center).frame(maxWidth: 330)
            }.padding(30).frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func albumChange(_ label: String, current: String, proposed: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(label).font(.caption).foregroundStyle(.secondary)
                Spacer()
                if !proposed.isEmpty && current != proposed { Label("Will Change", systemImage: "pencil").font(.caption).foregroundStyle(.secondary) }
            }
            if !proposed.isEmpty && current != proposed {
                Text(current.isEmpty ? "Empty or Multiple Values" : current).font(.caption).foregroundStyle(.secondary).strikethrough()
            }
            Text(proposed.isEmpty ? "Keep Current Value" : proposed).font(.callout)
        }
    }
    private func trackPair(_ local: LibraryDraft, proposed: AlbumLookupTrack) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(local.original.fileName).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text("\(proposed.disc).\(proposed.track)").font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                Text(proposed.title).font(.callout.weight(.medium))
                if local[.title] != proposed.title { Image(systemName: "pencil").font(.caption).foregroundStyle(.secondary) }
            }
            if local[.title] != proposed.title && !local[.title].isEmpty {
                Text("Previously: \(local[.title])").font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Divider().padding(.top, 4)
        }
    }
    private func majority(_ values: [String]) -> String {
        let values = values.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }
        let counts = Dictionary(grouping: values, by: { $0 }).mapValues(\.count)
        return counts.keys.sorted {
            if counts[$0] != counts[$1] { return counts[$0, default: 0] > counts[$1, default: 0] }
            return $0.localizedStandardCompare($1) == .orderedAscending
        }.first ?? ""
    }
    private func close() { request?.cancel(); dismiss() }
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
