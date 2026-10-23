use std::io::{Cursor, Read};

// Independent wire oracle: (//hp:tbl)[1]/hp:tr[1]/hp:tc/@header.
pub fn wire_headers(bytes: &[u8]) -> Vec<u32> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut xml).unwrap();
    let mut reader = quick_xml::Reader::from_str(&xml);
    let (mut tables, mut depth, mut target, mut first_row) = (0, 0_i32, None, false);
    let mut flags = Vec::new();
    loop {
        use quick_xml::events::Event;
        let event = reader.read_event().unwrap();
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                if e.local_name().as_ref() == "tbl" {
                    tables += 1;
                    if tables == 1 {
                        target = Some(depth);
                    }
                }
                if target == Some(depth - 1) && e.local_name().as_ref() == "tr" {
                    first_row = true;
                }
                if first_row && target == Some(depth - 2) && e.local_name().as_ref() == "tc" {
                    let a = e
                        .attributes()
                        .map(Result::unwrap)
                        .find(|a| a.key.as_ref() == "header")
                        .unwrap();
                    flags.push(a.value.parse().unwrap());
                }
                if !empty {
                    depth += 1;
                }
            }
            Event::End(e) => {
                depth -= 1;
                if first_row && target == Some(depth - 1) && e.local_name().as_ref() == "tr" {
                    return flags;
                }
            }
            Event::Eof => panic!("table 1 first row missing"),
            _ => {}
        }
    }
}
