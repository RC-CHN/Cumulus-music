use super::*;
use quick_xml::{Reader, events::Event};
pub(super) struct Node {
    pub name: String,
    pub text: String,
    pub children: Vec<Node>,
}
impl Node {
    pub fn field(&self, name: &str) -> Result<&str> {
        let mut nodes = self.children.iter().filter(|n| n.name == name);
        let field = nodes
            .next()
            .ok_or(Error::Response("missing OSS XML field"))?;
        if nodes.next().is_some() || field.text.is_empty() || !field.children.is_empty() {
            return Err(Error::Response("invalid or duplicate OSS XML field"));
        }
        Ok(&field.text)
    }
    pub fn scope(&self, a: &OssAllocation) -> Result<()> {
        if self.field("Bucket")? != a.bucket() || self.field("Key")? != a.object_key() {
            return Err(Error::Response("OSS response refers to another object"));
        }
        Ok(())
    }
}
pub(super) fn parse(raw: &[u8]) -> Result<Node> {
    if raw.is_empty() || raw.len() > 1024 * 1024 {
        return Err(Error::Response("empty or oversized OSS XML"));
    }
    let input = std::str::from_utf8(raw).map_err(|_| Error::Response("OSS XML is not UTF8"))?;
    let mut reader = Reader::from_str(input);
    let mut stack = Vec::<Node>::new();
    let mut root = None;
    let mut nodes = 0;
    loop {
        let event = reader
            .read_event()
            .map_err(|_| Error::Response("invalid OSS XML"))?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(start) | Event::Empty(start) => {
                nodes += 1;
                if stack.len() >= 16 || nodes > 50000 {
                    return Err(Error::Response("OSS XML structure exceeds limits"));
                }
                let name = std::str::from_utf8(start.local_name().as_ref())
                    .map_err(|_| Error::Response("invalid OSS XML name"))?
                    .to_owned();
                let node = Node {
                    name,
                    text: String::new(),
                    children: Vec::new(),
                };
                if empty {
                    append(&mut stack, &mut root, node)?;
                } else {
                    stack.push(node);
                }
            }
            Event::End(_) => {
                let node = stack.pop().ok_or(Error::Response("unbalanced OSS XML"))?;
                append(&mut stack, &mut root, node)?;
            }
            Event::Text(text) => {
                let text = text
                    .decode()
                    .map_err(|_| Error::Response("invalid OSS XML text"))?;
                if let Some(parent) = stack.last_mut() {
                    parent.text.push_str(&text);
                } else if !text.trim().is_empty() {
                    return Err(Error::Response("text outside OSS XML root"));
                }
            }
            Event::CData(text) => {
                let text = text
                    .decode()
                    .map_err(|_| Error::Response("invalid OSS XML CDATA"))?;
                stack
                    .last_mut()
                    .ok_or(Error::Response("CDATA outside OSS XML root"))?
                    .text
                    .push_str(&text);
            }
            Event::GeneralRef(reference) => {
                let text = reference
                    .decode()
                    .map_err(|_| Error::Response("invalid OSS XML reference"))?;
                let escaped = format!("&{text};");
                let value = quick_xml::escape::unescape(&escaped)
                    .map_err(|_| Error::Response("unsupported OSS XML entity"))?;
                stack
                    .last_mut()
                    .ok_or(Error::Response("entity outside OSS XML root"))?
                    .text
                    .push_str(&value);
            }
            Event::DocType(_) => {
                return Err(Error::Response("OSS XML document types are not accepted"));
            }
            Event::Decl(declaration) => {
                if let Some(encoding) = declaration.encoding() {
                    let value =
                        encoding.map_err(|_| Error::Response("invalid OSS XML declaration"))?;
                    if !value.eq_ignore_ascii_case(b"utf-8") {
                        return Err(Error::Response("unsupported OSS XML encoding"));
                    }
                }
            }
            Event::Comment(_) | Event::PI(_) => {}
            Event::Eof => break,
        }
    }
    if !stack.is_empty() {
        return Err(Error::Response("unclosed OSS XML element"));
    }
    root.ok_or(Error::Response("missing OSS XML root"))
}
fn append(stack: &mut [Node], root: &mut Option<Node>, node: Node) -> Result<()> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.replace(node).is_some() {
        return Err(Error::Response("multiple OSS XML roots"));
    }
    Ok(())
}
