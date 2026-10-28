#![allow(dead_code)]

pub(crate) fn attr<'a>(tag: &'a str, name: &str) -> &'a str {
    let needle = format!(" {name}=\"");
    let at = tag.find(&needle).unwrap_or_else(|| panic!("{name} in {tag}")) + needle.len();
    &tag[at..at + tag[at..].find('"').unwrap()]
}

/// Small parsed tree for wire assertions: exact qualified names, decoded
/// attribute values (absence stays None), ordered children, and text.
#[derive(Debug, Default)]
pub(crate) struct Element {
    pub name: String,
    pub attributes: std::collections::BTreeMap<String, String>,
    pub children: Vec<Element>,
    pub text: String,
}

impl Element {
    pub fn parse(xml: &str) -> Self {
        use quick_xml::{events::Event, Reader};
        let mut reader = Reader::from_str(xml);
        let mut stack = vec![Self::default()];
        loop {
            let event = reader.read_event().expect("valid XML");
            let empty = matches!(event, Event::Empty(_));
            match event {
                Event::Start(e) | Event::Empty(e) => {
                    let node = Self {
                        name: e.name().as_ref().to_string(),
                        attributes: e
                            .attributes()
                            .map(|a| {
                                let a = a.expect("attribute");
                                (
                                    a.key.as_ref().to_string(),
                                    a.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                                        .unwrap()
                                        .into_owned(),
                                )
                            })
                            .collect(),
                        ..Self::default()
                    };
                    if empty {
                        stack.last_mut().unwrap().children.push(node);
                    } else {
                        stack.push(node);
                    }
                }
                Event::End(_) => {
                    let node = stack.pop().unwrap();
                    stack.last_mut().unwrap().children.push(node);
                }
                Event::Text(e) => stack.last_mut().unwrap().text.push_str(&e.xml10_content()),
                Event::CData(e) => stack.last_mut().unwrap().text.push_str(&e.xml10_content()),
                // quick-xml reports `&amp;` and `&#xAC00;` apart from the text
                // around them; dropping them would cut the value.
                Event::GeneralRef(e) => {
                    let wire = format!("&{};", &*e);
                    let text = quick_xml::escape::unescape(&wire).expect("known reference");
                    stack.last_mut().unwrap().text.push_str(&text);
                }
                Event::Eof => break,
                _ => {}
            }
        }
        assert_eq!(stack.len(), 1);
        let mut document = stack.pop().unwrap();
        assert_eq!(document.children.len(), 1);
        document.children.pop().unwrap()
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }

    pub fn child_names(&self) -> Vec<&str> {
        self.children.iter().map(|c| c.name.as_str()).collect()
    }

    pub fn child(&self, name: &str) -> &Self {
        let nodes: Vec<_> = self.children.iter().filter(|c| c.name == name).collect();
        assert_eq!(nodes.len(), 1, "unique child {name} in {}", self.name);
        nodes[0]
    }

    pub fn find(&self, name: &str) -> &Self {
        if self.name == name {
            return self;
        }
        self.children
            .iter()
            .find_map(|c| c.find_optional(name))
            .unwrap_or_else(|| panic!("missing {name}"))
    }

    fn find_optional(&self, name: &str) -> Option<&Self> {
        if self.name == name {
            Some(self)
        } else {
            self.children.iter().find_map(|c| c.find_optional(name))
        }
    }
}
