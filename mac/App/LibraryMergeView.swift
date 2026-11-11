import SwiftUI

/// Gives the selected tracks another album's identity, without moving files.
struct LibraryMergeView: View {
    @ObservedObject var model: LibraryModel
    @Environment(\.dismiss) private var dismiss
    @State private var targetID: String?
    @Environment(\.controlActiveState) private var activeState

    private var targets: [LocalAlbum] { model.mergeTargets }
    private var selectedTarget: LocalAlbum? { targets.first { $0.id == targetID } }
    private var sourceCount: Int { model.selectedAlbumIDs.count }
    private var readOnlyCount: Int { model.selectedTracks.filter { !$0.original.writable }.count }

    var body: some View {
        VStack(spacing: 0) {
            PodSheetHeader(title: "Merge into Album",
                           subtitle: "\(model.selectedTracks.count) selected \(model.selectedTracks.count == 1 ? "track" : "tracks") from \(sourceCount) \(sourceCount == 1 ? "album" : "albums")")
            VStack(alignment: .leading, spacing: 14) {
                if targets.isEmpty {
                    PodCard(title: "No Other Album") {
                        Text(model.isRemote ? "Load a betterPod library containing several albums, then select the tracks you want to bring together."
                             : "Open a folder containing several albums, then select the tracks you want to bring together.")
                            .font(.callout).foregroundStyle(Pod.secondaryText)
                    }
                } else {
                    PodSectionTitle("Destination Album")
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(targets) { album in
                                destinationRow(album)
                            }
                        }
                    }
                    .frame(minHeight: 170, idealHeight: 230, maxHeight: 260)
                    .background(Color(nsColor: .textBackgroundColor))
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Pod.cardBorder))
                    .clipShape(RoundedRectangle(cornerRadius: 6))
                    .accessibilityLabel("Destination Album")
                    VStack(alignment: .leading, spacing: 6) {
                        Text("The selected tracks take the destination's album name and album artist. Their own artists and covers stay as they are. A missing album artist is filled for the whole album, with its performer or Various Artists.")
                        Text(model.isRemote ? "Review the draft, then save to update betterPod." : "Review the draft, then save. Audio files stay in their folders.")
                        if readOnlyCount > 0 {
                            Label("\(readOnlyCount) selected \(readOnlyCount == 1 ? "file is" : "files are") read-only and will be skipped.", systemImage: "lock")
                        }
                    }
                    .font(.caption).foregroundStyle(Pod.secondaryText)
                    .fixedSize(horizontal: false, vertical: true)
                }
                if let error = model.error {
                    Label(error, systemImage: "exclamationmark.triangle.fill")
                        .font(.callout).foregroundStyle(Pod.warning).fixedSize(horizontal: false, vertical: true)
                }
            }
            .padding(18)
            PodSheetFooter {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Merge into Album") {
                    guard let target = selectedTarget else { return }
                    model.mergeSelection(into: target.id)
                    if model.error == nil { dismiss() }
                }
                .buttonStyle(PodButtonStyle(kind: .primary))
                .keyboardShortcut(.defaultAction)
                .disabled(selectedTarget == nil || !model.canMergeSelection)
            }
            .buttonStyle(PodButtonStyle())
        }
        .frame(width: 560)
        .background(Pod.panel)
        .onAppear { targetID = targets.first?.id }
        .onChange(of: targets.map(\.id)) { _ in
            if !targets.contains(where: { $0.id == targetID }) { targetID = targets.first?.id }
        }
        .onChange(of: targetID) { _ in model.error = nil }
    }

    private func destinationRow(_ album: LocalAlbum) -> some View {
        let selected = album.id == targetID
        return HStack(spacing: 10) {
            Image(systemName: "square.stack").foregroundStyle(selected ? Color.white : Pod.accent)
            VStack(alignment: .leading, spacing: 1) {
                Text(album.title).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                Text(album.artist.isEmpty ? "Unknown Artist" : album.artist).font(.caption).lineLimit(1)
                    .foregroundStyle(selected ? Color.white.opacity(0.85) : Pod.secondaryText)
            }
            Spacer()
            Text("\(album.count) \(album.count == 1 ? "track" : "tracks")").font(.caption)
                .foregroundStyle(selected ? Color.white.opacity(0.85) : Pod.secondaryText)
        }
        .foregroundStyle(selected ? Color.white : Color.primary)
        .padding(.horizontal, 12).padding(.vertical, 6)
        .background { if selected { PodSelection(active: activeState != .inactive) } }
        .contentShape(Rectangle())
        .onTapGesture { targetID = album.id }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? [.isSelected, .isButton] : .isButton)
    }
}
