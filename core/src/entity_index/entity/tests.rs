use std::{collections::HashSet, path::PathBuf, time::Duration};

use indicatif::ProgressIterator;
use itertools::Itertools;

use crate::entrypoint::entity::EntityBuilder;

use super::{EntitySnippet, Span};

// fn fetch_wiki_src(name: &str) -> String {
//     let base = PathBuf::from("wiki-dump");
//     std::fs::create_dir(base);

//     if let Ok(cached) = std::fs::read(base.join(name)) {
//         return String::from_utf16_lossy(&lz_str::decompress(cached.as_slice()).unwrap());
//     }

//     let mut wait = Duration::from_millis(1500);
//     let text = loop {
//         std::thread::sleep(wait);
//         let text = reqwest::blocking::get(format!(
//             "https://en.wikipedia.org/wiki/{}?action=raw",
//             name.replace(' ', "_")
//         ))
//         .unwrap()
//         .text()
//         .unwrap();

//         if text.starts_with("<!DOCTYPE html>") {
//             eprintln!("retrying...");
//             wait *= 2;
//             continue;
//         }
//         break text;
//     };
// }

fn fix_info_thingy(span: &mut Span) {
    let prev_len = span.text.len();

    if let Some(start) = span.text.find(|c: char| !(c.is_whitespace() || c == '*')) {
        span.text.replace_range(0..start, "");
        let delta = prev_len - span.text.len();
        for link in &mut span.links {
            assert_eq!(start, delta);
            link.start = link.start.saturating_sub(start);
            link.end = link.end.saturating_sub(start);
        }
    }
}

#[test]
fn einstein() {
    let full_src = std::fs::read_to_string(
        "/Users/oeb25/Downloads/enwiki-20230920-pages-articles-multistream1.xml-p1p41242",
    )
    .unwrap();

    let count = full_src.split("<text").skip(1).count();

    for seg in full_src.split("<text").skip(1).progress_count(count as _) {
        let rest = seg.split_once('>').unwrap().1;
        let text = rest.split_once("</text>").unwrap().0;
        let mut eb = EntityBuilder::new();
        eb.append_title(text);
        eb.append_text(text);
        let entity = eb.build().unwrap();

        EntitySnippet::from_span(&entity.page_abstract, 300).to_md(None);

        for info in entity.info.values() {
            let mut span = info.clone();
            fix_info_thingy(&mut span);
            EntitySnippet::from_span(&span, 150).to_md(None);
        }
        for p in entity.paragraphs {
            EntitySnippet::from_span(&p.content, 300).to_md(None);
        }
    }

    // let mut queue = ["Albert Einstein", "Counter-Strike: Global Offensive"]
    //     .into_iter()
    //     .map(|s| s.to_string())
    //     .collect_vec();
    // let mut seen: HashSet<_> = queue.iter().cloned().collect();

    // while let Some(name) = queue.pop() {
    //     eprintln!("checking: {name}");
    //     let text = fetch_wiki_src(&name);
    //     let mut eb = EntityBuilder::new();
    //     eb.append_text(&text);
    //     let entity = eb.build().unwrap();

    //     EntitySnippet::from_span(&entity.page_abstract, usize::MAX).to_md(None);

    //     for p in entity.paragraphs {
    //         EntitySnippet::from_span(&p.content, usize::MAX).to_md(None);
    //     }

    //     for l in &entity.page_abstract.links {
    //         if seen.insert(l.target.clone()) {
    //             queue.push(l.target.clone())
    //         }
    //     }
    // }
}
