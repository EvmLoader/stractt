use criterion::{criterion_group, criterion_main, Criterion};
use cuely::{
    index::Index,
    searcher::{LocalSearcher, SearchQuery},
};

const INDEX_PATH: &str = "data/index";

macro_rules! bench {
    ($query:tt, $searcher:ident, $goggle:ident, $c:ident) => {
        let mut desc = "search '".to_string();
        desc.push_str($query);
        desc.push('\'');
        desc.push_str(" with goggle");
        $c.bench_function(desc.as_str(), |b| {
            b.iter(|| {
                $searcher
                    .search(&SearchQuery {
                        original: $query.to_string(),
                        selected_region: None,
                        goggle_program: Some($goggle.to_string()),
                        skip_pages: None,
                        site_rankings: None,
                    })
                    .unwrap()
            })
        });
    };
}

pub fn criterion_benchmark(c: &mut Criterion) {
    let index = Index::open(INDEX_PATH).unwrap();
    let searcher = LocalSearcher::new(index, None, None);
    let goggle = include_str!("../testcases/goggles/hacker_news.goggle");

    // for _ in 0..10 {
    bench!("the", searcher, goggle, c);
    bench!("dtu", searcher, goggle, c);
    bench!("the best", searcher, goggle, c);
    bench!("the circle of life", searcher, goggle, c);
    // }
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
