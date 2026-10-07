use anyhow::Result;
use quick_xml::{Reader, events::Event};

/// A minimal SVG DOM node. Building a tree (rather than streaming) lets us
/// resolve forward references (gradients in `<defs>`), inherit styles, and
/// keep the element/transform stack balanced by construction.
#[derive(Debug, Clone, Default)]
pub struct Node {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    pub text: String,
}

impl Node {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn children_named(&self, tag: &str) -> impl Iterator<Item = &Node> {
        self.children.iter().filter(move |c| c.tag == tag)
    }

    /// Parse an SVG document into a tree. The returned node is a synthetic
    /// root whose children include the `<svg>` element.
    pub fn build(input: &str) -> Result<Node> {
        let mut reader = Reader::from_str(input);
        reader.config_mut().trim_text(true);
        let mut root = Node {
            tag: "root".to_string(),
            ..Default::default()
        };
        let mut stack: Vec<Node> = Vec::new();

        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf)? {
                Event::Eof => break,
                Event::Start(e) => {
                    stack.push(read_attrs(&e, &reader)?);
                }
                Event::Empty(e) => {
                    let node = read_attrs(&e, &reader)?;
                    attach(&mut stack, &mut root, node);
                }
                Event::End(_) => {
                    if let Some(node) = stack.pop() {
                        attach(&mut stack, &mut root, node);
                    }
                }
                Event::Text(t) => {
                    if let Ok(decoded) = t.decode() {
                        push_text(&mut stack, &unescape(&decoded));
                    }
                }
                Event::CData(c) => {
                    if let Ok(decoded) = c.decode() {
                        push_text(&mut stack, &decoded);
                    }
                }
                _ => {}
            }
            buf.clear();
        }
        // Tolerate unclosed elements by folding whatever remains.
        while let Some(node) = stack.pop() {
            attach(&mut stack, &mut root, node);
        }
        Ok(root)
    }
}

fn read_attrs(
    e: &quick_xml::events::BytesStart,
    reader: &Reader<&[u8]>,
) -> Result<Node> {
    let mut node = Node {
        tag: local_name(e.name().as_ref()),
        ..Default::default()
    };
    for a in e.attributes() {
        let a = a?;
        let key = local_name(a.key.as_ref());
        let val = a
            .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, reader.decoder())?
            .into_owned();
        node.attrs.push((key, val));
    }
    Ok(node)
}

fn attach(stack: &mut Vec<Node>, root: &mut Node, node: Node) {
    match stack.last_mut() {
        Some(parent) => parent.children.push(node),
        None => root.children.push(node),
    }
}

fn push_text(stack: &mut Vec<Node>, text: &str) {
    let collapsed = collapse_ws(text);
    if collapsed.is_empty() {
        return;
    }
    if let Some(top) = stack.last_mut() {
        if !top.text.is_empty() {
            top.text.push(' ');
        }
        top.text.push_str(&collapsed);
    }
}

/// Resolve the five predefined XML entities plus numeric character references.
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(idx) = rest.find('&') {
        out.push_str(&rest[..idx]);
        rest = &rest[idx + 1..];
        let end = rest.find(';').unwrap_or(rest.len());
        let entity = &rest[..end];
        let replacement = match entity {
            "amp" => Some("&".to_string()),
            "lt" => Some("<".to_string()),
            "gt" => Some(">".to_string()),
            "quot" => Some("\"".to_string()),
            "apos" => Some("'".to_string()),
            e if e.starts_with("#x") || e.starts_with("#X") => {
                u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32).map(String::from)
            }
            e if e.starts_with('#') => e[1..]
                .parse::<u32>()
                .ok()
                .and_then(char::from_u32)
                .map(String::from),
            _ => None,
        };
        match replacement {
            Some(r) => {
                out.push_str(&r);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
            }
        }
    }
    out.push_str(rest);
    out
}

fn local_name(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    match s.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => s.into_owned(),
    }
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_nested_tree() {
        let dom = Node::build(
            r#"<svg><g id="a"><rect x="1"/></g><text>hi  there</text></svg>"#,
        )
        .unwrap();
        let svg = &dom.children[0];
        assert_eq!(svg.tag, "svg");
        assert_eq!(svg.children[0].tag, "g");
        assert_eq!(svg.children[0].attr("id"), Some("a"));
        assert_eq!(svg.children[0].children[0].tag, "rect");
        assert_eq!(svg.children[1].text, "hi there");
    }

    #[test]
    fn strips_namespace_prefixes() {
        let dom = Node::build(r#"<svg:svg xmlns:svg="x"><svg:rect/></svg:svg>"#).unwrap();
        assert_eq!(dom.children[0].tag, "svg");
        assert_eq!(dom.children[0].children[0].tag, "rect");
    }
}
