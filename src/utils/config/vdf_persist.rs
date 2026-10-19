//! Writes Lua-provided depot decryption keys into `<Steam>/config/config.vdf`
//! under a synthetic `apps/0/depots/<id>/DecryptionKey` section so that
//! external tools (like the Python blueprint extractor) can find them.
//!
//! ## Why synthetic `apps/0`
//!
//! At the moment `init_thread` runs (right after Lua parse), the hook layer
//! has not yet seen `BuildDepotDependency` for any app, so
//! `hook::manifest::lookup_depot` returns `None` for every depot. We do not
//! know which real app a given Lua depot belongs to. `0` is a valid u32 but
//! not a real Steam app, so Steam's config parser ignores the entry, while
//! the extractor's `InstallConfigStore -> Software -> Valve -> Steam ->
//! apps -> * -> depots -> * -> DecryptionKey` walk still picks it up.
//!
//! ## Skip-if-present
//!
//! If the depot already has a `DecryptionKey` somewhere in the tree (for
//! owned games Steam has already persisted), we leave that one alone. We
//! only add entries for depots Steam currently has no key for.
//!
//! ## #base safety
//!
//! `config.vdf` occasionally contains `#base "includedfile.vdf"` directives
//! at the top level. This parser does not read the included files, so
//! rewriting a config.vdf that uses `#base` would silently drop their
//! contents. We refuse to write in that case and log a warning.

use std::collections::HashSet;
use std::path::Path;

use super::lua_config;

// Ã¢â€â‚¬Ã¢â€â‚¬ VDF node Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

pub struct VdfNode {
    value: Option<String>,
    children: Vec<(String, VdfNode)>,
}

impl VdfNode {
    fn new_section() -> Self { Self { value: None, children: Vec::new() } }
    fn new_value(s: &str) -> Self { Self { value: Some(s.to_string()), children: Vec::new() } }

    fn child_index(&self, key: &str) -> Option<usize> {
        self.children.iter().position(|(k, _)| k.eq_ignore_ascii_case(key))
    }

    pub fn get(&self, key: &str) -> Option<&VdfNode> {
        self.child_index(key).map(|i| &self.children[i].1)
    }

    pub fn ensure_section(&mut self, key: &str) -> &mut VdfNode {
        if let Some(i) = self.child_index(key) {
            // Coerce a leaf that should be a section (should not happen
            // for a well-formed config.vdf).
            self.children[i].1.value = None;
            return &mut self.children[i].1;
        }
        self.children.push((key.to_string(), VdfNode::new_section()));
        let i = self.children.len() - 1;
        &mut self.children[i].1
    }

    fn set(&mut self, key: &str, value: &str) {
        if let Some(i) = self.child_index(key) {
            self.children[i].1.value = Some(value.to_string());
            self.children[i].1.children.clear();
        } else {
            self.children.push((key.to_string(), VdfNode::new_value(value)));
        }
    }
}

// Ã¢â€â‚¬Ã¢â€â‚¬ Tokenizer / parser Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

enum Tok { Str(String), OpenBrace, CloseBrace }

struct Lexer<'a> { text: &'a [u8], pos: usize }

impl<'a> Lexer<'a> {
    fn new(text: &'a str) -> Self { Self { text: text.as_bytes(), pos: 0 } }

    fn skip_ws(&mut self) {
        while self.pos < self.text.len() {
            let c = self.text[self.pos];
            if c.is_ascii_whitespace() {
                self.pos += 1;
            } else if c == b'/' && self.pos + 1 < self.text.len() && self.text[self.pos + 1] == b'/' {
                while self.pos < self.text.len() && self.text[self.pos] != b'\n' {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn next(&mut self) -> Option<Tok> {
        self.skip_ws();
        if self.pos >= self.text.len() { return None; }
        let c = self.text[self.pos];
        if c == b'{' { self.pos += 1; return Some(Tok::OpenBrace); }
        if c == b'}' { self.pos += 1; return Some(Tok::CloseBrace); }
        if c == b'"' {
            return self.read_quoted().map(Tok::Str);
        }
        self.read_unquoted().map(Tok::Str)
    }

    fn read_quoted(&mut self) -> Option<String> {
        self.pos += 1;
        let mut out: Vec<u8> = Vec::new();
        while self.pos < self.text.len() {
            let c = self.text[self.pos];
            self.pos += 1;
            if c == b'"' { return String::from_utf8(out).ok(); }
            if c == b'\\' && self.pos < self.text.len() {
                let e = self.text[self.pos];
                self.pos += 1;
                out.push(match e {
                    b'n' => b'\n', b't' => b'\t', b'r' => b'\r',
                    b'\\' => b'\\', b'"' => b'"',
                    _ => e,
                });
            } else {
                out.push(c);
            }
        }
        None
    }

    fn read_unquoted(&mut self) -> Option<String> {
        let start = self.pos;
        while self.pos < self.text.len() {
            let c = self.text[self.pos];
            if c.is_ascii_whitespace() || c == b'{' || c == b'}' { break; }
            self.pos += 1;
        }
        if start == self.pos { return None; }
        String::from_utf8(self.text[start..self.pos].to_vec()).ok()
    }
}

fn parse_vdf(text: &str) -> Option<VdfNode> {
    let mut lex = Lexer::new(text);
    let mut stack: Vec<(String, VdfNode)> = vec![(String::new(), VdfNode::new_section())];

    while let Some(tok) = lex.next() {
        match tok {
            Tok::Str(key) => {
                match lex.next()? {
                    Tok::OpenBrace => {
                        stack.push((key, VdfNode::new_section()));
                    }
                    Tok::Str(value) => {
                        stack.last_mut()?.1.set(&key, &value);
                    }
                    Tok::CloseBrace => return None,
                }
            }
            Tok::CloseBrace => {
                if stack.len() <= 1 { return None; }
                let (k, node) = stack.pop()?;
                stack.last_mut()?.1.children.push((k, node));
            }
            Tok::OpenBrace => return None,
        }
    }

    if stack.len() != 1 { return None; }
    Some(stack.into_iter().next()?.1)
}

fn serialize_vdf(node: &VdfNode) -> String {
    let mut out = String::new();
    write_node(&mut out, node, 0);
    out
}

fn write_node(out: &mut String, node: &VdfNode, indent: usize) {
    let pad = "\t".repeat(indent);
    for (k, v) in &node.children {
        if let Some(val) = &v.value {
            out.push_str(&format!("{}\"{}\" \"{}\"\n", pad, escape(k), escape(val)));
        } else {
            out.push_str(&format!("{}\"{}\"\n{}{{\n", pad, escape(k), pad));
            write_node(out, v, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
        }
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

// Ã¢â€â‚¬Ã¢â€â‚¬ Walk helpers Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

fn collect_depot_ids(node: &VdfNode, out: &mut HashSet<u32>) {
    if let Some(depots) = node.get("depots") {
        for (k, v) in &depots.children {
            if let Ok(id) = k.parse::<u32>() {
                if let Some(key) = v.get("DecryptionKey") {
                    let has = key.value.as_ref().map(|s| !s.is_empty()).unwrap_or(false);
                    if has { out.insert(id); }
                }
            }
        }
    }
    for (_, child) in &node.children {
        collect_depot_ids(child, out);
    }
}

// Ã¢â€â‚¬Ã¢â€â‚¬ Public entry point Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

/// Read `<steam_root>/config/config.vdf`, inject every Lua-provided
/// depot key that is not already present, and write the file back
/// atomically. Returns `true` if the file was updated.
pub fn persist_lua_keys_to_config_vdf(steam_root: &str) -> bool {
    let config_path = Path::new(steam_root).join("config").join("config.vdf");

    let text = match std::fs::read_to_string(&config_path) {
        Ok(t) => t,
        Err(e) => {
            LOG_WARN!("VdfPersist: cannot read {}: {}", config_path.display(), e);
            return false;
        }
    };

    let mut root = match parse_vdf(&text) {
        Some(r) => r,
        None => {
            LOG_WARN!("VdfPersist: failed to parse {}", config_path.display());
            return false;
        }
    };

    // Refuse to rewrite a file that uses #base directives; we would lose
    // the included content.
    if root.children.iter().any(|(k, _)| k.eq_ignore_ascii_case("#base")) {
        LOG_WARN!(
            "VdfPersist: config.vdf contains #base directives; refusing to \
             rewrite (would lose included content). Let Steam rewrite it once \
             (change any setting in Steam), then retry."
        );
        return false;
    }

    // Collect every depot Steam already has a key for. Any depot in that
    // set is skipped, so real owned games are never touched.
    let mut existing: HashSet<u32> = HashSet::new();
    collect_depot_ids(&root, &mut existing);

    let mut to_add: Vec<(u32, String)> = Vec::new();
    for depot_id in lua_config::get_all_depot_ids() {
        if existing.contains(&depot_id) { continue; }
        let key = lua_config::get_decryption_key(depot_id);
        if key.is_empty() { continue; }
        to_add.push((depot_id, hex::encode(&key)));
    }

    if to_add.is_empty() {
        LOG_INFO!(
            "VdfPersist: no new depot key(s) to add ({} already present in config.vdf)",
            existing.len()
        );
        return true;
    }

    // Sanity: the structure must look like a real config.vdf. Refuse to
    // rewrite the file if it is not, rather than creating a half-tree.
    let structure_ok = root.get("InstallConfigStore")
        .and_then(|n| n.get("Software"))
        .and_then(|n| n.get("Valve"))
        .and_then(|n| n.get("Steam"))
        .is_some();
    if !structure_ok {
        LOG_WARN!(
            "VdfPersist: unexpected config.vdf structure (missing \
             InstallConfigStore/Software/Valve/Steam), refusing to write"
        );
        return false;
    }

    // Walk down to apps/0/depots, creating nodes as needed.
    let mut cur = &mut root;
    for key in ["InstallConfigStore", "Software", "Valve", "Steam"] {
        cur = cur.ensure_section(key);
    }
    let apps = cur.ensure_section("apps");
    let synth_app = apps.ensure_section("0");
    let synth_depots = synth_app.ensure_section("depots");

    for (depot_id, key_hex) in &to_add {
        let depot_node = synth_depots.ensure_section(&depot_id.to_string());
        depot_node.set("DecryptionKey", key_hex);
    }

    let output = serialize_vdf(&root);

    // Atomic write.
    let tmp = config_path.with_extension("vdf.rustytools.tmp");
    if let Err(e) = std::fs::write(&tmp, &output) {
        LOG_WARN!("VdfPersist: write temp failed: {}", e);
        return false;
    }
    if let Err(e) = std::fs::rename(&tmp, &config_path) {
        LOG_WARN!("VdfPersist: rename failed: {}", e);
        let _ = std::fs::remove_file(&tmp);
        return false;
    }

    LOG_INFO!(
        "VdfPersist: added {} depot key(s) to config.vdf ({} already present)",
        to_add.len(),
        existing.len()
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_roundtrip() {
        let src = r#"
"InstallConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "apps"
                {
                    "730"
                    {
                        "depots"
                        {
                            "731" { "DecryptionKey" "abcd" }
                        }
                    }
                }
            }
        }
    }
}
"#;
        let root = parse_vdf(src).expect("parse");
        let mut out = HashSet::new();
        collect_depot_ids(&root, &mut out);
        assert!(out.contains(&731));

        let serialized = serialize_vdf(&root);
        let root2 = parse_vdf(&serialized).expect("reparse");
        let mut out2 = HashSet::new();
        collect_depot_ids(&root2, &mut out2);
        assert!(out2.contains(&731));
    }

    #[test]
    fn inject_new_depot() {
        let mut root = parse_vdf(r#"
"InstallConfigStore" { "Software" { "Valve" { "Steam" { "apps" { } } } } }
"#).unwrap();
        let apps = root
            .ensure_section("InstallConfigStore")
            .ensure_section("Software")
            .ensure_section("Valve")
            .ensure_section("Steam")
            .ensure_section("apps");
        let zero = apps.ensure_section("0");
        let depots = zero.ensure_section("depots");
        depots.ensure_section("12345").set("DecryptionKey", "deadbeef");
        let s = serialize_vdf(&root);
        let mut out = HashSet::new();
        collect_depot_ids(&parse_vdf(&s).unwrap(), &mut out);
        assert!(out.contains(&12345));
    }

    #[test]
    fn skips_existing_depot() {
        let mut root = parse_vdf(r#"
"InstallConfigStore" { "Software" { "Valve" { "Steam" { "apps" {
    "730" { "depots" { "731" { "DecryptionKey" "abcd" } } }
} } } }
"#).unwrap();
        let mut existing = HashSet::new();
        collect_depot_ids(&root, &mut existing);
        assert!(existing.contains(&731));

        // Simulate the skip path: only write if the depot is not already
        // present. Result must not contain a duplicate "wrong" key.
        let apps = root
            .ensure_section("InstallConfigStore")
            .ensure_section("Software")
            .ensure_section("Valve")
            .ensure_section("Steam")
            .ensure_section("apps");
        for id in [731u32] {
            if !existing.contains(&id) {
                let zero = apps.ensure_section("0");
                let depots = zero.ensure_section("depots");
                depots.ensure_section(&id.to_string()).set("DecryptionKey", "wrong");
            }
        }
        let s = serialize_vdf(&root);
        assert!(!s.contains("\"wrong\""));
    }
}
