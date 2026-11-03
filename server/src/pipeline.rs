//! Narration pipelines: node graphs of how a video gets its subtitles and voices (run by the narrator worker, muxed
//! by narration.rs). Two built-in presets — "Default · Whisper" and "Default · OCR" — plus presets the admin saves;
//! every job keeps a copy of the graph it was queued with.
//!
//! Nodes: sources `whisper` (lang or "auto"), `ocr` (burned-in subtitles; lang, crop = bottom share of the frame),
//! `subtitle` (the video's own); `translate` (to lang); `voice` (vieneu | edge, voice name); `mux` (one per graph).
//! Wires: a source or translate feeds translate or voice nodes; voices feed the mux.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Serialize, Deserialize, Clone)]
pub struct Graph {
    pub nodes: Vec<Node>,
    /// (from, to) node ids.
    pub edges: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub lang: String,
    /// voice: "vieneu" (Vietnamese only) or "edge" (edge-tts, many languages).
    #[serde(default)]
    pub engine: String,
    /// voice: preset voice; empty = the default from Settings / for the language.
    #[serde(default)]
    pub voice: String,
    /// ocr: share of the frame height, from the bottom, where the subtitles are.
    #[serde(default)]
    pub crop: f64,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
}

fn makes_subtitles(kind: &str) -> bool {
    matches!(kind, "whisper" | "ocr" | "subtitle" | "translate")
}

fn node(id: &str, kind: &str, lang: &str, x: f64, y: f64) -> Node {
    Node { id: id.into(), kind: kind.into(), lang: lang.into(), engine: String::new(), voice: String::new(), crop: 0.0, x, y }
}

fn voice(id: &str, engine: &str, x: f64, y: f64) -> Node {
    Node { engine: engine.into(), ..node(id, "voice", "", x, y) }
}

fn edges(e: &[(&str, &str)]) -> Vec<(String, String)> {
    e.iter().map(|(a, b)| ((*a).into(), (*b).into())).collect()
}

/// (id, name, graph) of the built-in presets.
pub fn builtins() -> Vec<(&'static str, &'static str, Graph)> {
    let whisper = Graph {
        nodes: vec![
            node("src", "whisper", "auto", 40.0, 150.0),
            node("en", "translate", "en", 320.0, 40.0),
            node("vi", "translate", "vi", 320.0, 150.0),
            node("zh", "translate", "zh", 320.0, 260.0),
            voice("vvi", "vieneu", 600.0, 150.0),
            voice("vzh", "edge", 600.0, 260.0),
            node("mux", "mux", "", 880.0, 205.0),
        ],
        edges: edges(&[("src", "en"), ("src", "vi"), ("src", "zh"), ("vi", "vvi"), ("zh", "vzh"), ("vvi", "mux"), ("vzh", "mux")]),
    };
    let ocr = Graph {
        nodes: vec![
            Node { crop: 0.35, ..node("src", "ocr", "vi", 40.0, 150.0) },
            node("en", "translate", "en", 320.0, 40.0),
            node("zh", "translate", "zh", 320.0, 260.0),
            voice("vvi", "vieneu", 600.0, 150.0),
            voice("vzh", "edge", 600.0, 260.0),
            node("mux", "mux", "", 880.0, 205.0),
        ],
        edges: edges(&[("src", "en"), ("src", "zh"), ("src", "vvi"), ("zh", "vzh"), ("vvi", "mux"), ("vzh", "mux")]),
    };
    vec![("whisper", "Default · Whisper", whisper), ("ocr", "Default · OCR (burned-in subtitles)", ocr)]
}

/// What a graph does, for the queue: (spoken/source description, voice languages).
#[derive(Debug)]
pub struct Summary {
    pub source: String,
    pub voices: Vec<String>,
}

/// Checks a graph; `langs` are the subtitle languages known to narration.rs.
pub fn validate(g: &Graph, langs: &[&str]) -> Result<Summary, String> {
    let mut by_id = HashMap::new();
    for n in &g.nodes {
        if n.id.is_empty() || by_id.insert(n.id.as_str(), n).is_some() {
            return Err(format!("node id {:?} is empty or used twice", n.id));
        }
        let lang_ok = langs.contains(&n.lang.as_str());
        match n.kind.as_str() {
            "whisper" if !(n.lang == "auto" || lang_ok) => return Err(format!("Whisper: unknown language {:?}", n.lang)),
            "ocr" | "subtitle" | "translate" if !lang_ok => return Err(format!("{}: pick a language", n.kind)),
            "ocr" if !(n.crop == 0.0 || (0.05..=0.8).contains(&n.crop)) => return Err("OCR: the crop must be 5–80 % of the frame".into()),
            "voice" if !matches!(n.engine.as_str(), "vieneu" | "edge") => return Err("voice: pick VieNeu or edge-tts".into()),
            "whisper" | "ocr" | "subtitle" | "translate" | "voice" | "mux" => {}
            k => return Err(format!("unknown node type {k:?}")),
        }
    }
    let mut inputs: HashMap<&str, Vec<&Node>> = HashMap::new();
    for (a, b) in &g.edges {
        let (Some(from), Some(to)) = (by_id.get(a.as_str()), by_id.get(b.as_str())) else {
            return Err(format!("a wire joins a missing node ({a} → {b})"));
        };
        let ok = match to.kind.as_str() {
            "translate" | "voice" => makes_subtitles(&from.kind),
            "mux" => from.kind == "voice",
            _ => false,
        };
        if !ok {
            return Err(format!("{} cannot feed {}", from.kind, to.kind));
        }
        inputs.entry(b.as_str()).or_default().push(from);
    }
    for n in &g.nodes {
        let k = inputs.get(n.id.as_str()).map_or(0, Vec::len);
        match n.kind.as_str() {
            "translate" | "voice" if k != 1 => return Err(format!("{} needs exactly one input", n.kind)),
            "mux" if k == 0 => return Err("the mux needs at least one voice".into()),
            _ => {}
        }
    }
    if g.nodes.iter().filter(|n| n.kind == "mux").count() > 1 {
        return Err("only one mux per pipeline".into());
    }
    let sources: Vec<&Node> = g.nodes.iter().filter(|n| matches!(n.kind.as_str(), "whisper" | "ocr" | "subtitle")).collect();
    if sources.is_empty() {
        return Err("add a source: Whisper, OCR or the video's subtitles".into());
    }
    // No cycles: repeatedly remove nodes without inputs.
    let mut indeg: HashMap<&str, usize> = g.nodes.iter().map(|n| (n.id.as_str(), 0)).collect();
    for (_, b) in &g.edges {
        *indeg.get_mut(b.as_str()).unwrap() += 1;
    }
    let mut ready: Vec<&str> = indeg.iter().filter(|(_, d)| **d == 0).map(|(id, _)| *id).collect();
    let mut seen = 0;
    while let Some(id) = ready.pop() {
        seen += 1;
        for (_, b) in g.edges.iter().filter(|(a, _)| a == id) {
            let d = indeg.get_mut(b.as_str()).unwrap();
            *d -= 1;
            if *d == 0 {
                ready.push(b.as_str());
            }
        }
    }
    if seen != g.nodes.len() {
        return Err("the wires make a loop".into());
    }
    // Language a node's subtitles are in ("" when Whisper detects it).
    let lang_of = |n: &Node| -> String {
        match n.kind.as_str() {
            "whisper" if n.lang == "auto" => String::new(),
            _ => n.lang.clone(),
        }
    };
    let mut voices = Vec::new();
    let mut seen_langs = HashSet::new();
    for v in g.nodes.iter().filter(|n| n.kind == "voice") {
        let lang = lang_of(inputs[v.id.as_str()][0]);
        if v.engine == "vieneu" && !lang.is_empty() && lang != "vi" {
            return Err(format!("VieNeu reads Vietnamese only, this voice gets {lang}"));
        }
        if !lang.is_empty() && !seen_langs.insert(lang.clone()) {
            return Err(format!("two voices in {lang}: the video gets one track per language"));
        }
        voices.push(if lang.is_empty() { "auto".into() } else { lang });
    }
    let source = sources
        .iter()
        .map(|s| match s.kind.as_str() {
            "whisper" => format!("whisper {}", s.lang),
            k => format!("{k} {}", s.lang),
        })
        .collect::<Vec<_>>()
        .join(" + ");
    Ok(Summary { source, voices })
}

#[cfg(test)]
mod tests {
    use super::*;
    const LANGS: &[&str] = &["en", "vi", "zh", "ko"];

    #[test]
    fn builtins_are_valid() {
        for (_, name, g) in builtins() {
            let s = validate(&g, LANGS).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(s.voices.len(), 2, "{name}");
        }
    }

    #[test]
    fn rejects_bad_graphs() {
        let (_, _, mut g) = builtins().remove(0);
        g.edges.push(("mux".into(), "vi".into()));
        assert!(validate(&g, LANGS).unwrap_err().contains("cannot feed"));
        let (_, _, mut g) = builtins().remove(0);
        g.edges.push(("en".into(), "vvi".into()));
        assert!(validate(&g, LANGS).unwrap_err().contains("exactly one input"));
        let (_, _, mut g) = builtins().remove(1);
        g.edges.retain(|(a, b)| !(a == "src" && b == "vvi"));
        g.edges.push(("zh".into(), "vvi".into()));
        assert!(validate(&g, LANGS).unwrap_err().contains("VieNeu"));
        let (_, _, mut g) = builtins().remove(0);
        g.nodes.retain(|n| n.kind != "whisper");
        g.edges.retain(|(a, _)| a != "src");
        assert!(validate(&g, LANGS).is_err());
    }
}
