import SwiftUI

struct DeviceLibraryView: View {
    @ObservedObject var model: DeviceModel
    @ObservedObject var library: LibraryModel

    init(model: DeviceModel) {
        self.model = model
        self.library = model.library
    }

    var body: some View {
        VStack(spacing: 0) {
            if model.connected {
                HStack(spacing: 14) {
                    Label(library.location, systemImage: "wifi").font(.headline)
                    Text(model.progress.isEmpty ? "Connected over your local network" : model.progress)
                        .font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Button("Refresh") { model.refresh() }.disabled(model.busy)
                    Button("Disconnect") { model.disconnect() }.disabled(library.busy)
                }.padding(.horizontal, 20).padding(.vertical, 12)
                Divider()
                if library.remoteSession?.supportsAlbumJoining != true {
                    Label("Update betterPod Rust to use Magic and merge albums over Wi-Fi. Manual metadata and artwork editing are still available.", systemImage: "info.circle")
                        .font(.callout).foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading).padding(12)
                }
                if library.remoteNeedsReload {
                    HStack {
                        Label("The library needs to be reloaded before saving. Refresh, or disconnect and connect again.", systemImage: "exclamationmark.triangle")
                            .font(.callout)
                        Spacer()
                    }.padding(12).background(Color.orange.opacity(0.1))
                }
                LibraryView(model: library)
            } else {
                connectionForm
            }
            if let error = model.error {
                HStack(alignment: .top) {
                    Text(error).font(.callout).foregroundStyle(.red).textSelection(.enabled)
                    Spacer()
                    Button { model.error = nil } label: { Image(systemName: "xmark") }.buttonStyle(.plain)
                        .accessibilityLabel("Dismiss connection error")
                }.padding(16).background(Color.red.opacity(0.05))
            }
        }
    }

    private var connectionForm: some View {
        VStack(spacing: 24) {
            Spacer()
            Image(systemName: "wifi").font(.system(size: 48, weight: .light)).foregroundStyle(Color.accentColor)
            VStack(spacing: 8) {
                Text("Your betterPod library, on your Mac.").font(.title2.weight(.semibold))
                Text("On betterPod Rust, open Music → Wi-Fi Library and start a connection.\nKeep both devices on the same Wi-Fi network, then enter its eight-digit code.")
                    .foregroundStyle(.secondary).multilineTextAlignment(.center)
            }
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Text("Nearby betterPods").font(.headline)
                    Spacer()
                    if model.discovering { ProgressView().controlSize(.small) }
                    Button("Find betterPod") { model.discover() }.disabled(model.discovering || model.connecting)
                }
                if !model.devices.isEmpty {
                    Picker("Device", selection: $model.selectedDevice) {
                        ForEach(model.devices) { device in Text("\(device.name) · \(device.host)").tag(device.id) }
                    }
                    .disabled(model.connecting)
                } else {
                    Text(model.discovering ? "Looking on your local network…" : "Start the connection on betterPod, then choose Find betterPod.")
                        .font(.callout).foregroundStyle(.secondary)
                }
                HStack {
                    TextField("0000 0000", text: $model.code)
                        .textFieldStyle(.roundedBorder).font(.system(size: 25, design: .monospaced))
                        .frame(width: 210).accessibilityLabel("Eight-digit connection code")
                        .onSubmit { model.connect() }
                        .disabled(model.connecting)
                    Spacer()
                    if model.connecting {
                        Button("Cancel") { model.disconnect(askToDiscard: false) }
                    } else {
                        Button("Connect") { model.connect() }.buttonStyle(.borderedProminent)
                            .disabled(!model.validCode)
                    }
                }
                DisclosureGroup("Enter a local address manually", isExpanded: $model.useManualAddress) {
                    HStack {
                        TextField("IP address", text: $model.manualHost).textFieldStyle(.roundedBorder)
                        TextField("Port", text: $model.manualPort).textFieldStyle(.roundedBorder).frame(width: 80)
                    }.padding(.top, 8).disabled(model.connecting)
                    Text("Use the address and port shown on betterPod if your network blocks discovery.")
                        .font(.caption).foregroundStyle(.secondary).padding(.top, 4)
                }
                .font(.callout)
                .onChange(of: model.useManualAddress) { expanded in
                    if !expanded { model.manualHost = ""; model.manualPort = "" }
                }
                if model.connecting {
                    HStack {
                        ProgressView().controlSize(.small)
                        Text(model.progress).font(.callout).foregroundStyle(.secondary)
                    }
                }
            }
            .padding(22)
            .frame(width: 510)
            .background(Color(nsColor: .controlBackgroundColor))
            .clipShape(RoundedRectangle(cornerRadius: 12))
            Text("Review changes before saving them to betterPod.\nThe connection stays inside your local network.")
                .font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center)
            Spacer()
        }
        .padding(30)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
