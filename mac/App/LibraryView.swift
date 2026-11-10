import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct LibraryView: View {
    @ObservedObject var model: LibraryModel
    @Environment(\.undoManager) private var undoManager
    @State private var targeted = false
    private var rows: [LibraryDraft] { model.visibleTracks }
    private var canEdit: Bool { model.editableCount > 0 && !model.busy && !model.remoteNeedsReload }

    var body: some View {
        VStack(spacing: 0) {
            if model.tracks.isEmpty {
                emptyState
            } else {
                HSplitView {
                    trackList.frame(minWidth: 490, maxWidth: .infinity, maxHeight: .infinity)
                    inspector.frame(minWidth: 270, idealWidth: 300, maxWidth: 360, maxHeight: .infinity)
                }
            }
            if let error = model.error {
                notice(error, isError: true)
            } else if let message = model.message {
                notice(message, isError: false)
            }
            footer
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .navigationTitle(model.isRemote ? "betterPod Library" : "Library")
        .navigationSubtitle(model.location.isEmpty ? "Albums on this Mac" : model.location)
        .toolbar { libraryToolbar }
        .overlay {
            if targeted {
                RoundedRectangle(cornerRadius: 10)
                    .strokeBorder(Color.accentColor, lineWidth: 3)
                    .padding(6).allowsHitTesting(false)
            }
        }
        .onDrop(of: [.fileURL], isTargeted: $targeted) { providers in
            guard !model.busy, !model.isRemote else { return false }
            loadURLs(providers) { model.open($0) }
            return true
        }
        .sheet(isPresented: $model.showingLookup) {
            AlbumLookupView(model: model, targetIDs: model.selection)
        }
        .sheet(isPresented: $model.showingMerge) {
            LibraryMergeView(model: model)
        }
        .sheet(isPresented: $model.showingMagic) {
            MagicLibraryView(model: model)
        }
        .onChange(of: model.selection) { _ in model.requestArtwork() }
        .onChange(of: model.changesOnly) { _ in
            model.selection.formIntersection(Set(rows.map(\.id)))
        }
    }

    @ToolbarContentBuilder private var libraryToolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .automatic) {
            if !model.isRemote {
                Button { model.choose() } label: { Label("Open Folder", systemImage: "folder.badge.plus") }
                    .help("Open an Album Folder or Audio Files (⌘O)")
                    .disabled(model.busy)
                Button { model.clearLibrary(undoManager: undoManager) } label: {
                    Label("Clear Library", systemImage: "xmark.circle")
                }
                .help("Remove All Tracks from This View; Files Stay on Disk")
                .disabled(model.busy || model.tracks.isEmpty)
            }
            Button { model.beginMagic() } label: { Label("Magic", systemImage: "wand.and.stars") }
                .buttonStyle(AppleProminentButtonStyle())
                .help("Automatically Identify, Group and Fix the Selected Tracks")
                .disabled(!model.canRunMagic)
            Menu {
                Button("Find Album Details…") { model.beginLookup() }.disabled(!canEdit)
                Button("Find Cover…") { model.beginLookup(artworkOnly: true) }.disabled(!canEdit)
                Button("Merge into Album…") { model.beginMerge() }
                    .disabled(!model.canMergeSelection)
                if !model.isRemote {
                    Divider()
                    Button("Remove Selected Tracks", systemImage: "minus.circle") { model.removeSelection(undoManager: undoManager) }
                        .disabled(model.selection.isEmpty)
                    Button("Clear Library", systemImage: "clear") { model.clearLibrary(undoManager: undoManager) }
                        .disabled(model.tracks.isEmpty)
                    if model.canUndoRemoval {
                        Divider()
                        Button("Undo Library Removal") { model.undoRemoval() }
                    }
                }
            } label: { Label("Manual Tools", systemImage: "slider.horizontal.3") }
            .help(model.isRemote ? "Find Metadata and Artwork or Merge Albums on betterPod" : "Find Metadata and Artwork, Merge Albums, or Remove Tracks from This View")
            .disabled(model.busy || model.remoteNeedsReload)
        }
        ToolbarItem(placement: .primaryAction) {
            Button { model.save() } label: {
                Label(model.hasChanges ? "Save Changes (\(model.dirtyCount))" : "Save Changes", systemImage: "checkmark")
            }
            .labelStyle(.titleAndIcon)
            .buttonStyle(AppleProminentButtonStyle())
            .disabled(!model.canSave)
            .help(model.isRemote ? "Save Changed Tags and Artwork on betterPod (⌘S)" : "Save Changed Tags and Artwork to Your Files (⌘S)")
        }
    }

    private var emptyState: some View {
        VStack(spacing: 18) {
            Image(systemName: "opticaldisc")
                .font(.system(size: 56, weight: .regular))
                .foregroundStyle(.tertiary)
            VStack(spacing: 8) {
                Text(model.isRemote ? "No Music on betterPod" : "Organize Your Music")
                    .font(.title2.weight(.semibold))
                Text(model.isRemote ? "Add music and rescan on betterPod, then refresh the library." : "Open an album folder to find its details, correct track tags,\nand choose the right cover.")
                    .foregroundStyle(.secondary).multilineTextAlignment(.center)
            }
            if !model.isRemote {
                Button("Open Album Folder…") { model.choose() }
                    .buttonStyle(AppleProminentButtonStyle()).controlSize(.large)
                    .disabled(model.busy)
                Text("Or drag a folder here. Removing it later keeps your files on disk.")
                    .font(.callout).foregroundStyle(.secondary)
                if model.canUndoRemoval {
                    Button("Undo Library Removal") { model.undoRemoval() }
                }
            }
            if model.busy { ProgressView("Reading Audio Files…").controlSize(.small) }
            Text("MP3 · FLAC · Ogg Vorbis · M4A\nWAV and AIFF are read-only.")
                .font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity).padding(32)
    }

    private var trackList: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                Picker("Album", selection: Binding(get: { model.albumFilter }, set: {
                    model.albumFilter = $0
                    model.filterChanged()
                })) {
                    Text("All Tracks (\(model.tracks.count))").tag("")
                    ForEach(model.albums) { album in
                        Text("\(album.title) — \(album.artist) (\(album.count))").tag(album.id)
                    }
                }
                .labelsHidden().frame(maxWidth: 340)
                Button {
                    model.showingAlbumSelection = true
                } label: { Label("Albums", systemImage: "square.stack") }
                    .controlSize(.small).disabled(model.busy)
                    .help("Select Several Whole Albums")
                    .popover(isPresented: $model.showingAlbumSelection) {
                        LibraryAlbumSelectionView(model: model) { model.changesOnly = false }
                    }
                Spacer(minLength: 4)
                Toggle(isOn: $model.changesOnly) { Label("Changes", systemImage: "pencil.line") }
                    .toggleStyle(.button).controlSize(.small)
                    .help("Show Tracks with Edited or Saved Metadata or Artwork")
            }
            .padding(.horizontal, 14).padding(.vertical, 10)
            Table(rows, selection: $model.selection, sortOrder: $model.sortOrder) {
                TableColumn("#", value: \.sortTrack) { row in
                    LibraryMetadataCell(row: row, field: .track, selected: model.selection.contains(row.id))
                }.width(34)
                TableColumn("Cover", value: \.sortArtwork) { row in
                    LibraryCover(data: row.cover)
                        .frame(width: 34, height: 34)
                        .overlay(alignment: .bottomTrailing) {
                            if row.artworkChangeState != .unchanged {
                                Image(systemName: row.artworkChangeState == .modified ? "pencil.circle.fill" : "checkmark.circle.fill")
                                    .symbolRenderingMode(.palette)
                                    .foregroundStyle(.white, row.artworkChangeState == .modified ? Color.accentColor : .green)
                                    .font(.system(size: 13)).offset(x: 4, y: 4)
                            }
                        }
                        .help(artworkHelp(row))
                        .accessibilityLabel(artworkHelp(row))
                }.width(42)
                TableColumn("Title", value: \.sortTitle) { row in
                    VStack(alignment: .leading, spacing: 3) {
                        LibraryMetadataCell(row: row, field: .title, selected: model.selection.contains(row.id), fallback: row.original.fileName)
                        Text(row.original.fileName).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                    }.padding(.vertical, 5)
                }.width(min: 140, ideal: 220)
                TableColumn("Artist", value: \.sortArtist) { row in
                    LibraryMetadataCell(row: row, field: .artist, selected: model.selection.contains(row.id))
                }.width(min: 80, ideal: 120)
                TableColumn("Album", value: \.sortAlbum) { row in
                    LibraryMetadataCell(row: row, field: .album, selected: model.selection.contains(row.id), display: row.sortAlbum)
                }.width(min: 90, ideal: 145)
                TableColumn("Changes", value: \.sortChanges) { row in
                    VStack(alignment: .leading, spacing: 3) {
                        if row.dirty {
                            LibraryChangeBadge(state: .modified, selected: model.selection.contains(row.id))
                        } else if row.hasRecordedChanges {
                            LibraryChangeBadge(state: .saved, selected: model.selection.contains(row.id))
                        }
                        if row.hasRecordedChanges {
                            Text(changeSummary(row)).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                        }
                    }.help(changeSummary(row))
                }.width(min: 80, ideal: 115)
            }
            .disabled(model.busy)
            .onDeleteCommand {
                if !model.isRemote { model.removeSelection(undoManager: undoManager) }
            }
            .contextMenu(forSelectionType: String.self) { ids in
                Button("Magic") {
                    model.selection = ids
                    model.beginMagic()
                }.disabled(!canEdit(ids))
                Divider()
                Button("Find Album Details…") {
                    model.selection = ids
                    model.beginLookup()
                }.disabled(!canEdit(ids))
                Button("Find Cover…") {
                    model.selection = ids
                    model.beginLookup(artworkOnly: true)
                }.disabled(!canEdit(ids))
                Divider()
                Button("Merge Selected Tracks into Album…") {
                    model.selection = ids
                    model.beginMerge()
                }.disabled(!canEdit(ids))
                if !model.isRemote {
                    Button("Remove Selected Tracks from Library") {
                        model.selection = ids
                        model.removeSelection(undoManager: undoManager)
                    }.disabled(model.busy || ids.isEmpty)
                }
            }
        }
    }

    private var inspector: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                HStack {
                    Text(model.selection.count == 1 ? "Track Details" : "\(model.selection.count) Tracks Selected").font(.headline)
                    Spacer()
                }
                if model.selection.isEmpty {
                    Text("Select a track to see its details. Hold ⌘ or ⇧ to select several tracks.")
                        .foregroundStyle(.secondary)
                } else {
                    artwork
                    Divider()
                    HStack {
                        Text("Metadata").font(.headline)
                        Spacer()
                        Button { model.beginLookup() } label: { Image(systemName: "sparkle.magnifyingglass") }
                            .help("Find Album Details").accessibilityLabel("Find Album Details")
                            .disabled(!canEdit)
                    }
                    if selectedAlbumGroupingState != .unchanged {
                        HStack {
                            Text("Album Grouping").font(.callout).foregroundStyle(.secondary)
                            LibraryChangeBadge(state: selectedAlbumGroupingState, compact: true)
                        }
                        .help("The selected tracks' album membership is included when saving to betterPod.")
                    }
                    VStack(alignment: .leading, spacing: 12) {
                        ForEach(LibraryField.allCases) { field in
                            VStack(alignment: .leading, spacing: 5) {
                                HStack(spacing: 6) {
                                    Text(field.label).font(.callout).foregroundStyle(.secondary)
                                    LibraryChangeBadge(state: selectedState(field), compact: true)
                                    Spacer()
                                    if model.isMixed(field) {
                                        Button("Clear") { model.set(field, to: "") }
                                            .font(.caption).buttonStyle(.link)
                                            .accessibilityLabel("Clear \(field.label) for Selected Tracks")
                                    }
                                }
                                TextField(model.isMixed(field) ? "Multiple Values" : field.label,
                                          text: Binding(get: { model.commonValue(field) }, set: { model.set(field, to: $0) }))
                                    .textFieldStyle(.roundedBorder)
                                    .accessibilityLabel(field.label)
                                    .help(selectedHelp(field))
                            }
                        }
                    }
                    .disabled(!canEdit)
                    if model.editableCount < model.selection.count {
                        Label("\(model.selection.count - model.editableCount) selected files are read-only. Edits apply to the others.", systemImage: "lock")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    if model.selectedTracks.contains(where: \.dirty) {
                        Button("Revert Unsaved Changes") { model.revertSelection() }.disabled(model.busy)
                    }
                    if let row = model.selectedTracks.first, model.selection.count == 1 {
                        if let album = row.original.displayAlbum, album != row.original.album || !(row.original.albumGroup ?? "").isEmpty {
                            Text("Album shown on betterPod: \(album). Editing Album or Album Artist also updates its grouping.")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        if let warning = row.original.warning { Text(warning).font(.caption).foregroundStyle(.secondary) }
                        Text("\(row.original.format.uppercased()) · \(humanBytes(row.original.size))")
                            .font(.caption).foregroundStyle(.secondary)
                        Text(row.id).font(.caption).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                }
            }.padding(20)
        }
        .background(Color(nsColor: .controlBackgroundColor))
    }

    private var artwork: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 14) {
                LibraryCover(data: model.selectedTracks.first?.cover)
                    .frame(width: 110, height: 110)
                    .overlay {
                        if selectedArtworkState != .unchanged {
                            RoundedRectangle(cornerRadius: 7).strokeBorder(selectedArtworkState == .modified ? Color.accentColor : .green, lineWidth: 2)
                        }
                    }
                VStack(alignment: .leading, spacing: 8) {
                    Text("Artwork").font(.headline)
                    LibraryChangeBadge(state: selectedArtworkState)
                    if model.artworkLoading { ProgressView().controlSize(.small) }
                    Button("Find Cover…") { model.beginLookup(artworkOnly: true) }.disabled(!canEdit)
                    Menu("Change Artwork") {
                        Button("Choose Image…") { model.chooseArtwork() }
                        Button("Remove Embedded Cover") { model.setArtwork(.remove) }
                    }.disabled(!canEdit)
                }
            }
            if model.isRemote && model.selection.count > 1 {
                Text("Preview of the first selected track. Changes apply to every selected editable track.")
                    .font(.caption).foregroundStyle(.secondary)
            } else if let first = model.selectedTracks.first, model.selectedTracks.dropFirst().contains(where: { $0.cover != first.cover }) {
                Text("Selected tracks have different covers.").font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private var selectedArtworkState: LibraryChangeState {
        if model.selectedTracks.contains(where: { $0.artworkChangeState == .modified }) { return .modified }
        if model.selectedTracks.contains(where: { $0.artworkChangeState == .saved }) { return .saved }
        return .unchanged
    }
    private var selectedAlbumGroupingState: LibraryChangeState {
        if model.selectedTracks.contains(where: { $0.albumGroupingChangeState == .modified }) { return .modified }
        if model.selectedTracks.contains(where: { $0.albumGroupingChangeState == .saved }) { return .saved }
        return .unchanged
    }
    private func canEdit(_ ids: Set<String>) -> Bool {
        !model.busy && !model.remoteNeedsReload && model.tracks.contains { ids.contains($0.id) && $0.original.writable }
    }
    private func selectedState(_ field: LibraryField) -> LibraryChangeState {
        if model.selectedTracks.contains(where: { $0.changeState(for: field) == .modified }) { return .modified }
        if model.selectedTracks.contains(where: { $0.changeState(for: field) == .saved }) { return .saved }
        return .unchanged
    }
    private func selectedHelp(_ field: LibraryField) -> String {
        guard let row = model.selectedTracks.first, model.selection.count == 1 else { return "Changes apply to every selected editable track." }
        return metadataHelp(row, field)
    }
    private func changeSummary(_ row: LibraryDraft) -> String {
        let fields = row.changedFields.union(row.savedChangedFields)
        var names = LibraryField.allCases.filter(fields.contains).map(\.label)
        if row.albumGroupingChangeState != .unchanged { names.append("Album Grouping") }
        if row.artworkChangeState != .unchanged { names.append("Artwork") }
        return names.joined(separator: ", ")
    }
    private func artworkHelp(_ row: LibraryDraft) -> String {
        switch row.artworkChangeState {
        case .modified: return "Artwork edited · Not yet saved"
        case .saved: return "Updated artwork saved to this file"
        case .unchanged: return "Artwork unchanged"
        }
    }
    private func notice(_ text: String, isError: Bool) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: isError ? "exclamationmark.triangle" : "info.circle")
                .foregroundStyle(isError ? Color.orange : .secondary)
            LibraryNoticeText(text: text)
            Button { model.error = nil; model.message = nil } label: { Image(systemName: "xmark") }
                .buttonStyle(.plain).accessibilityLabel("Dismiss Message")
        }
        .padding(12).background(Color(nsColor: .controlBackgroundColor))
    }
    private var footer: some View {
        HStack(spacing: 14) {
            Label(model.busy ? "Working…" : "\(rows.count) Tracks · \(model.albums.count) Albums", systemImage: model.isRemote ? "wifi" : "internaldrive")
            Spacer()
            if model.tracks.contains(where: \.hasRecordedChanges) {
                Label("Edited", systemImage: "pencil.circle").foregroundStyle(Color.accentColor)
                LibraryChangeBadge(state: .saved)
            }
        }
        .font(.caption).foregroundStyle(.secondary).padding(.horizontal, 16).padding(.vertical, 9)
    }
}

private struct LibraryNoticeText: View {
    let text: String
    @State private var contentHeight: CGFloat = 18

    var body: some View {
        ScrollView {
            Text(text).font(.callout).textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background {
                    GeometryReader { geometry in
                        Color.clear.preference(key: LibraryNoticeHeight.self, value: geometry.size.height)
                    }
                }
        }
        .frame(height: min(75, max(18, contentHeight)))
        .onPreferenceChange(LibraryNoticeHeight.self) { contentHeight = $0 }
    }
}

private struct LibraryNoticeHeight: PreferenceKey {
    static let defaultValue: CGFloat = 18
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) { value = max(value, nextValue()) }
}

private func metadataHelp(_ row: LibraryDraft, _ field: LibraryField) -> String {
    switch row.changeState(for: field) {
    case .unchanged: return field.label + " unchanged"
    case .modified: return "\(field.label): \(row.previousValue(for: field).isEmpty ? "Empty" : row.previousValue(for: field)) → \(row[field].isEmpty ? "Empty" : row[field]) · Not yet saved"
    case .saved: return "\(field.label) updated and saved · Previously: \(row.previousValue(for: field).isEmpty ? "Empty" : row.previousValue(for: field))"
    }
}

private struct LibraryMetadataCell: View {
    let row: LibraryDraft
    let field: LibraryField
    var selected = false
    var fallback = ""
    var display: String? = nil
    private var state: LibraryChangeState { row.changeState(for: field) }
    var body: some View {
        HStack(spacing: 4) {
            Text((display ?? row[field]).isEmpty ? fallback : (display ?? row[field]))
                .fontWeight(state == .unchanged ? .regular : .medium).lineLimit(1)
            if state != .unchanged {
                Image(systemName: state == .modified ? "pencil" : "checkmark")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(selected ? Color.primary : (state == .modified ? .accentColor : .green))
            }
        }
        .padding(.vertical, 3)
        .help(metadataHelp(row, field))
        .accessibilityElement(children: .combine)
        .accessibilityLabel(metadataHelp(row, field))
    }
}

struct LibraryChangeBadge: View {
    let state: LibraryChangeState
    var compact = false
    var selected = false
    var body: some View {
        if state != .unchanged {
            Label {
                Text(state == .modified ? "Edited" : "Saved").foregroundStyle(selected ? .primary : .secondary)
            } icon: {
                Image(systemName: state == .modified ? "pencil.circle" : "checkmark.circle")
                    .foregroundStyle(selected ? Color.primary : (state == .modified ? .accentColor : .green))
            }
                .font(compact ? .caption2 : .caption)
        }
    }
}

struct LibraryCover: View {
    let data: Data?
    var body: some View {
        Group {
            if let data, let image = NSImage(data: data) {
                Image(nsImage: image).resizable().scaledToFit()
            } else {
                ZStack {
                    RoundedRectangle(cornerRadius: 7).fill(Color.secondary.opacity(0.1))
                    Image(systemName: "music.note").font(.title2).foregroundStyle(.secondary)
                }
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 7))
    }
}

private struct LibraryAlbumSelectionView: View {
    @ObservedObject var model: LibraryModel
    var onSelected: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var selected = Set<String>()

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Select Albums").font(.headline)
            Text("Use ⌘ or ⇧ to select several albums, then run Magic or edit their tracks together.")
                .font(.callout).foregroundStyle(.secondary)
            List(model.albums, selection: $selected) { album in
                HStack(spacing: 10) {
                    Image(systemName: "square.stack").foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 3) {
                        Text(album.title).lineLimit(1)
                        Text(album.artist).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer()
                    Text("\(album.count)").font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                }.padding(.vertical, 4).tag(album.id)
            }.listStyle(.inset)
            HStack {
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Spacer()
                Button("Select \(selected.count) \(selected.count == 1 ? "Album" : "Albums")") {
                    onSelected()
                    model.selectAlbums(selected)
                    dismiss()
                }
                .buttonStyle(AppleProminentButtonStyle())
                .disabled(selected.isEmpty)
                .keyboardShortcut(.defaultAction)
            }
        }.padding(18).frame(width: 420, height: 420)
            .onAppear { selected = model.selectedAlbumIDs }
    }
}
