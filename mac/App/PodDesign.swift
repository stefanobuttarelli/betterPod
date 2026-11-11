// The Mac app's look: betterPod's glossy header and blue selection from the
// console (src/ij_theme.go, src/ij_views.go), the Share studio's light greys,
// and native tables, fields and menus inside. Surfaces are drawn, never
// translucent, so the app looks the same on every macOS version.

import AppKit
import SwiftUI

enum Pod {
    static func rgb(_ red: Int, _ green: Int, _ blue: Int, _ alpha: CGFloat = 1) -> NSColor {
        NSColor(srgbRed: CGFloat(red) / 255, green: CGFloat(green) / 255, blue: CGFloat(blue) / 255, alpha: alpha)
    }
    /// A color with a light and a dark appearance.
    static func color(_ light: NSColor, _ dark: NSColor) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? dark : light
        })
    }

    // The console header (#FEFFFF → #B1B6B9, border #7995A3), softened for a taller bar.
    static let chromeTop = color(rgb(253, 254, 254), rgb(76, 79, 84))
    static let chromeBottom = color(rgb(203, 208, 212), rgb(42, 44, 48))
    static let chromeBorder = color(rgb(132, 150, 161), rgb(10, 11, 12))
    static let chromeHighlight = color(rgb(255, 255, 255, 0.95), rgb(255, 255, 255, 0.12))
    static let chromeText = color(rgb(32, 35, 40), rgb(240, 241, 243))

    // The source list: the Share studio's blue-grey.
    static let sidebarTop = color(rgb(234, 238, 242), rgb(38, 40, 44))
    static let sidebarBottom = color(rgb(220, 226, 232), rgb(29, 30, 33))
    static let sidebarBorder = color(rgb(172, 182, 192), rgb(8, 9, 10))
    static let sectionText = color(rgb(104, 113, 124), rgb(142, 147, 154))

    // The console's selection cursor and its inactive grey.
    static let selection = [Color(nsColor: rgb(60, 184, 255)), Color(nsColor: rgb(52, 122, 181))]
    static let selectionInactive = [Color(nsColor: rgb(168, 176, 186)), Color(nsColor: rgb(122, 131, 142))]
    static let selectionBorder = Color(nsColor: rgb(42, 104, 158))
    static let accent = Color(nsColor: rgb(40, 124, 200))

    // Panels and cards beside the table.
    static let panel = color(rgb(242, 244, 247), rgb(31, 32, 35))
    static let card = color(rgb(255, 255, 255), rgb(42, 44, 48))
    static let cardBorder = color(rgb(208, 213, 219), rgb(62, 64, 69))
    static let strip = color(rgb(236, 239, 243), rgb(37, 39, 42))
    static let hairline = color(rgb(196, 202, 209), rgb(12, 13, 15))
    static let secondaryText = color(rgb(104, 112, 122), rgb(150, 153, 158))

    // The console's sheet buttons (#FFFFFF → #CBCCCE → #DFDFDF), lighter on a Mac.
    static let buttonTop = color(rgb(255, 255, 255), rgb(96, 99, 105))
    static let buttonMiddle = color(rgb(240, 241, 243), rgb(70, 73, 78))
    static let buttonLower = color(rgb(226, 228, 231), rgb(58, 61, 66))
    static let buttonBottom = color(rgb(236, 238, 240), rgb(64, 67, 72))
    static let buttonBorder = color(rgb(150, 158, 168), rgb(16, 17, 20))
    static let buttonText = color(rgb(30, 33, 38), rgb(238, 239, 241))

    static let edited = color(rgb(40, 124, 200), rgb(90, 170, 245))
    static let saved = color(rgb(40, 150, 70), rgb(80, 200, 110))
    static let warning = color(rgb(200, 110, 20), rgb(245, 165, 70))

    static let headerHeight: CGFloat = 52
    static let sidebarWidth: CGFloat = 196
}

/// The glossy bar behind headers, status bars and sheet titles.
struct PodChrome: View {
    var border: Edge = .bottom

    var body: some View {
        LinearGradient(colors: [Pod.chromeTop, Pod.chromeBottom], startPoint: .top, endPoint: .bottom)
            .overlay(alignment: .top) { Pod.chromeHighlight.frame(height: 1).padding(.top, border == .top ? 1 : 0) }
            .overlay(alignment: border == .top ? .top : .bottom) { Pod.chromeBorder.frame(height: 1) }
    }
}

/// The console's blue cursor: a two-stop gradient with a light top edge.
struct PodSelection: View {
    var active = true

    var body: some View {
        LinearGradient(colors: active ? Pod.selection : Pod.selectionInactive, startPoint: .top, endPoint: .bottom)
            .overlay(alignment: .top) { Color.white.opacity(0.28).frame(height: 1) }
            .overlay(alignment: .bottom) { (active ? Pod.selectionBorder : Color.black.opacity(0.15)).frame(height: 1) }
    }
}

/// Buttons as on the console's sheets: glossy grey, or the selection blue for
/// the main action of a screen.
struct PodButtonStyle: ButtonStyle {
    enum Kind { case normal, primary }
    var kind: Kind = .normal
    var selected = false
    var compact = false
    var large = false
    @Environment(\.isEnabled) private var enabled
    @Environment(\.controlActiveState) private var activeState
    @Environment(\.colorSchemeContrast) private var contrast

    func makeBody(configuration: Configuration) -> some View {
        let shape = RoundedRectangle(cornerRadius: compact ? 5 : 6, style: .continuous)
        let blue = (kind == .primary || selected) && enabled
        return configuration.label
            .font(.system(size: large ? 14 : (compact ? 11.5 : 12.5), weight: kind == .primary ? .semibold : .medium))
            .labelStyle(PodButtonLabelStyle())
            .lineLimit(1)
            .foregroundStyle(blue ? Color.white : Pod.buttonText)
            .padding(.horizontal, large ? 18 : (compact ? 9 : 12))
            .frame(minHeight: large ? 30 : (compact ? 21 : 25))
            .background {
                if blue {
                    LinearGradient(colors: activeState == .inactive && !selected ? Pod.selectionInactive : Pod.selection,
                                   startPoint: .top, endPoint: .bottom)
                        .overlay(Color.black.opacity(configuration.isPressed ? 0.18 : 0))
                } else {
                    LinearGradient(stops: [.init(color: Pod.buttonTop, location: 0), .init(color: Pod.buttonMiddle, location: 0.5),
                                           .init(color: Pod.buttonLower, location: 0.5), .init(color: Pod.buttonBottom, location: 1)],
                                   startPoint: .top, endPoint: .bottom)
                        .overlay(Color.black.opacity(configuration.isPressed ? 0.12 : 0))
                }
            }
            .clipShape(shape)
            .overlay(shape.strokeBorder(blue ? Pod.selectionBorder : Pod.buttonBorder.opacity(contrast == .increased ? 1 : 0.85), lineWidth: 1))
            .overlay(alignment: .top) {
                Color.white.opacity(blue ? 0.3 : 0.6).frame(height: 1).padding(.horizontal, 3).padding(.top, 1).allowsHitTesting(false)
            }
            .shadow(color: .black.opacity(enabled ? 0.12 : 0), radius: 0.5, y: 0.5)
            .opacity(enabled ? 1 : 0.45)
            .contentShape(shape)
    }
}

private struct PodButtonLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 5) {
            configuration.icon.imageScale(.small)
            configuration.title
        }
    }
}

/// A rounded white card with a hairline, for inspector and sheet sections.
struct PodCard<Content: View>: View {
    var title: String?
    var padding: CGFloat = 14
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let title { PodSectionTitle(title) }
            content
        }
        .padding(padding)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Pod.card, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(Pod.cardBorder, lineWidth: 1))
    }
}

/// Small capitals, as the classic source list's section names.
struct PodSectionTitle: View {
    let text: String
    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text.uppercased())
            .font(.system(size: 10.5, weight: .semibold))
            .tracking(0.6)
            .foregroundStyle(Pod.sectionText)
            .accessibilityAddTraits(.isHeader)
    }
}

/// A one-line message under a header: information, or a problem in orange.
struct PodBanner<Actions: View>: View {
    let text: String
    var isError = false
    var onDismiss: (() -> Void)?
    @ViewBuilder var actions: Actions

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: isError ? "exclamationmark.triangle.fill" : "info.circle.fill")
                .foregroundStyle(isError ? Pod.warning : Pod.edited)
            Text(text).font(.callout).textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
            actions
            if let onDismiss {
                Button(action: onDismiss) { Image(systemName: "xmark").font(.system(size: 10, weight: .bold)) }
                    .buttonStyle(.plain).foregroundStyle(Pod.secondaryText)
                    .accessibilityLabel("Dismiss Message")
            }
        }
        .padding(.horizontal, 14).padding(.vertical, 9)
        .background(Pod.strip)
        .overlay(alignment: .bottom) { Pod.hairline.frame(height: 1) }
    }
}

extension PodBanner where Actions == EmptyView {
    init(text: String, isError: Bool = false, onDismiss: (() -> Void)? = nil) {
        self.init(text: text, isError: isError, onDismiss: onDismiss) { EmptyView() }
    }
}

/// A sheet's glossy title bar, centered as on the console's header.
struct PodSheetHeader<Accessory: View>: View {
    let title: String
    var subtitle: String?
    @ViewBuilder var accessory: Accessory

    var body: some View {
        ZStack {
            PodChrome()
            VStack(spacing: 1) {
                Text(title).font(.system(size: 13, weight: .bold)).foregroundStyle(Pod.chromeText)
                if let subtitle { Text(subtitle).font(.system(size: 11)).foregroundStyle(Pod.secondaryText).lineLimit(1) }
            }
            HStack { Spacer(); accessory }.padding(.horizontal, 14)
        }
        .frame(height: subtitle == nil ? 34 : 42)
    }
}

extension PodSheetHeader where Accessory == EmptyView {
    init(title: String, subtitle: String? = nil) {
        self.init(title: title, subtitle: subtitle) { EmptyView() }
    }
}

/// A sheet's bottom bar, for its buttons.
struct PodSheetFooter<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        HStack(spacing: 10) { content }
            .padding(.horizontal, 16).padding(.vertical, 11)
            .background(Pod.strip)
            .overlay(alignment: .top) { Pod.hairline.frame(height: 1) }
    }
}

/// Lets the drawn header move the window, as a title bar does, and zoom it
/// on a double click.
struct WindowDragArea: NSViewRepresentable {
    final class DragView: NSView {
        override var mouseDownCanMoveWindow: Bool { true }
        override func mouseDown(with event: NSEvent) {
            if event.clickCount == 2 {
                let action = UserDefaults.standard.string(forKey: "AppleActionOnDoubleClick") ?? "Maximize"
                if action == "Minimize" { window?.performMiniaturize(nil) }
                else if action != "None" { window?.performZoom(nil) }
            } else {
                window?.performDrag(with: event)
            }
        }
    }

    func makeNSView(context: Context) -> DragView { DragView() }
    func updateNSView(_ view: DragView, context: Context) {}
}
