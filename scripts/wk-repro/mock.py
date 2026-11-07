import http.server, json, os, random, socketserver, sys, time, urllib.parse
DIST = sys.argv[2]
random.seed(11)
T = ["Introduction to FreeCAD & It's Benefits in Engineering", "FreeCAD Installation & Overview of FreeCAD Interface", "Things Anyone can do with FreeCAD", "Let's Explore Basics of FreeCAD",
     "Customize Default FreeCAD User Interface Settings", "Create Keyboard Shortcut in FreeCAD", "Install Addon Offline in FreeCAD", "Reset User Preferences in FreeCAD",
     "Redefine Sketch to New Reference in FreeCAD Sketcher", "Reorient Sketch with offset in FreeCAD", "Merge Multiple Sketch into Single Sketch", "Types of Arc in FreeCAD Sketcher Workbench"]
iid = [100]
def folder(name, n, sub=()):
    items = []
    for k in range(n):
        iid[0] += 1
        items.append({"id": iid[0], "library_id": 1, "title": f"{k + 1:03d} {T[(k + len(name)) % len(T)]}", "duration": 300 + 60 * k, "width": 1920, "height": 1080,
                      "img": f"/api/items/{iid[0]}/img/", "imgv": 1, "color": "#2b3350", "position": 0, "played": k % 6 == 0})
    return {"name": name, "path": name, "folders": list(sub), "items": items}
FOLDERS = [("01 - Introduction to FreeCAD & What is New in FreeCAD 1.0", 4), ("02 - Getting started with FreeCAD 1.0 & it's Customization", 7),
           ("03 - Fundamental of Sketching in FreeCAD 1.0", 16), ("04 - Fundamental of Part Design Workbench in FreeCAD", 18), ("05 - Sketch based features", 9),
           ("06 - Pad, pocket and revolve", 13), ("07 - Boolean operations", 7), ("08 - Assembly workbench basics", 25), ("09 - TechDraw", 6), ("10 - Advanced modelling projects", 19)]
TREE = {"name": "", "path": "", "folders": [folder(n, c) for n, c in FOLDERS], "items": []}
LIBS = [{"id": i + 1, "name": n, "kind": "videos", "count": 50, "img": None, "imgv": None, "color": "#2b3350"} for i, n in enumerate(["FreeCAD Course", "3Blue1Brown", "12 Days of Pink Panther", "The Adventure Of TinTin", "Âm Mưu ATHENA - 2011", "American Dragon Jake Long", "Anh Em Nhà Dalton", "Anime", "Arrow Season 1", "Arrow Season 2"])]
LIBS[0]["count"] = sum(c for _, c in FOLDERS)
class H(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **k): super().__init__(*a, directory=DIST, **k)
    def log_message(self, *a): pass
    def j(self, o, code=200):
        b = json.dumps(o).encode(); self.send_response(code); self.send_header("Content-Type", "application/json"); self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
    def do_GET(self):
        p = urllib.parse.urlparse(self.path).path
        if p.startswith("/api/items/") and "/img/" in p:
            time.sleep(random.uniform(0.1, 1.5))                 # thumbnails over a VPN
            data = open(f"{os.path.dirname(__file__)}/img/{int(p.split('/')[3]) % 14}.webp", "rb").read()
            self.send_response(200); self.send_header("Content-Type", "image/webp"); self.send_header("Content-Length", str(len(data))); self.send_header("Cache-Control", "max-age=3600"); self.end_headers(); self.wfile.write(data); return
        if p == "/api/health": return self.j({"app": "kellyfin", "version": "2.19.0"})
        if p == "/api/me": return self.j({"id": 1, "name": "owner", "is_admin": True})
        if p == "/api/libraries": return self.j(LIBS)
        if p == "/api/libraries/1": return self.j(LIBS[0])
        if p == "/api/libraries/1/tree": time.sleep(0.4); return self.j(TREE)
        if p.startswith("/api/"): return self.j({}, 404)
        if not os.path.exists(os.path.join(DIST, p.lstrip("/"))): self.path = "/index.html"
        super().do_GET()
class S(socketserver.ThreadingMixIn, http.server.HTTPServer): daemon_threads = True
S(("0.0.0.0", int(sys.argv[1])), H).serve_forever()
