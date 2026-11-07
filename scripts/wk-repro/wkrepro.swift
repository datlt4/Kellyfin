import Cocoa
import WebKit

// Loads a page in a WKWebView (the engine of the Tauri macOS app), scrolls it, and screenshots only this window.
let args = CommandLine.arguments
let url = args[1], out = args[2]
let zoom = args.count > 3 ? Double(args[3]) ?? 1.0 : 1.0
let steps = args.count > 4 ? Int(args[4]) ?? 8 : 8
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let conf = WKWebViewConfiguration()
let ucc = WKUserContentController()
ucc.addUserScript(WKUserScript(source: "localStorage.setItem('token','t'); localStorage.setItem('libView','tree');", injectionTime: .atDocumentStart, forMainFrameOnly: true))
// Experiments: extra CSS (arg 5) and/or JS (arg 6) injected before the page runs.
if args.count > 5, let css = try? String(contentsOfFile: args[5], encoding: .utf8), !css.isEmpty {
    let js = "document.addEventListener('DOMContentLoaded', function(){ var s = document.createElement('style'); s.textContent = " + (String(data: try! JSONSerialization.data(withJSONObject: [css], options: []), encoding: .utf8)!.dropFirst().dropLast()) + "; document.head.appendChild(s); });"
    ucc.addUserScript(WKUserScript(source: js, injectionTime: .atDocumentStart, forMainFrameOnly: true))
}
if args.count > 6, let js = try? String(contentsOfFile: args[6], encoding: .utf8), !js.isEmpty {
    ucc.addUserScript(WKUserScript(source: js, injectionTime: .atDocumentStart, forMainFrameOnly: true))
}
conf.userContentController = ucc
let rect = NSRect(x: 0, y: 40, width: 1536, height: 860)
let win = NSWindow(contentRect: rect, styleMask: [.titled], backing: .buffered, defer: false)
let web = WKWebView(frame: rect, configuration: conf)
web.pageZoom = CGFloat(zoom)
win.contentView = web
win.title = "Kellyfin WKWebView repro (closes itself)"
win.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps: true)
web.load(URLRequest(url: URL(string: url)!))

func shot(_ n: Int) {
    // The window's own composited pixels (what the screen shows), taken inside this process: no screen-recording permission needed.
    guard let cg = CGWindowListCreateImage(.null, .optionIncludingWindow, CGWindowID(win.windowNumber), [.boundsIgnoreFraming, .bestResolution]) else { print("no image"); return }
    let rep = NSBitmapImageRep(cgImage: cg)
    try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: "\(out)_\(n).png"))
}
var n = 0
Timer.scheduledTimer(withTimeInterval: 4.0, repeats: true) { t in
    if n == 0 {
        web.evaluateJavaScript("[window.innerWidth, window.devicePixelRatio, document.querySelectorAll('.card').length, getComputedStyle(document.querySelector('.grid')||document.body).gridTemplateColumns.split(' ').length].join(' ')") { r, _ in
            print("innerWidth dpr cards columns:", r ?? "n/a"); fflush(stdout)
        }
    }
    shot(n); n += 1
    if n > steps { exit(0) }
    web.evaluateJavaScript("window.scrollBy(0, Math.round(window.innerHeight * 0.75))", completionHandler: nil)
}
RunLoop.main.run()
