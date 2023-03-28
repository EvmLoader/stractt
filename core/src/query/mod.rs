// Stract is an open source web search engine.
// Copyright (C) 2023 Stract ApS
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use crate::{
    inverted_index::InvertedIndex,
    ranking::{Signal, SignalCoefficient},
    search_ctx::Ctx,
    searcher::SearchQuery,
    webpage::region::Region,
    Result,
};
use optics::{ast::RankingTarget, Optic, SiteRankings};
use std::{collections::HashMap, str::FromStr};
use tantivy::query::{BooleanQuery, Occur, QueryClone};

mod const_query;
pub mod intersection;
pub mod optic;
pub mod parser;
mod pattern_query;
pub mod union;

use parser::Term;

use self::optic::AsMultipleTantivyQuery;

const MAX_SIMILAR_TERMS: usize = 10;

#[derive(Clone, Debug)]
pub struct Query {
    #[allow(clippy::vec_box)]
    terms: Vec<Box<Term>>,
    simple_terms_text: Vec<String>,
    tantivy_query: Box<BooleanQuery>,
    site_rankings: SiteRankings,
    offset: usize,
    region: Option<Region>,
    optic: Option<Optic>,
    top_n: usize,
}

impl Query {
    pub fn parse(ctx: &Ctx, query: &SearchQuery, index: &InvertedIndex) -> Result<Query> {
        let parsed_terms = parser::parse(&query.query);

        let mut term_count = HashMap::new();
        let mut terms = Vec::new();

        for term in parsed_terms {
            let count = term_count.entry(term.clone()).or_insert(0);

            if *count < MAX_SIMILAR_TERMS {
                terms.push(term);
            }

            *count += 1;
        }

        let schema = index.schema();
        let tokenizer_manager = index.tokenizers();

        let fields: Vec<(tantivy::schema::Field, &tantivy::schema::FieldEntry)> =
            schema.fields().collect();

        let queries: Vec<(Occur, Box<dyn tantivy::query::Query + 'static>)> = terms
            .iter()
            .flat_map(|term| term.as_tantivy_query(&fields, tokenizer_manager))
            .collect();

        let simple_terms_text: Vec<String> = terms
            .iter()
            .filter_map(|term| {
                if let Term::Simple(term) = term.as_ref() {
                    Some(term.clone())
                } else {
                    None
                }
            })
            .collect();

        let mut tantivy_query = Box::new(BooleanQuery::new(queries));

        let mut optic = query
            .site_rankings
            .clone()
            .map(|site_rankings| site_rankings.into_optic());

        if let Some(optic_program) = &query.optic_program {
            match optic.as_mut() {
                Some(inner) => {
                    optic = Some(inner.clone().try_merge(Optic::parse(optic_program)?)?);
                }
                None => {
                    optic = Some(Optic::parse(optic_program)?);
                }
            }
        }

        if let Some(optic) = &optic {
            let mut subqueries = vec![(Occur::Must, tantivy_query.box_clone())];
            subqueries.append(&mut optic.as_multiple_tantivy(&schema, &ctx.fastfield_reader));
            tantivy_query = Box::new(BooleanQuery::new(subqueries));
        }

        Ok(Query {
            terms,
            site_rankings: optic
                .as_ref()
                .map(|optic| optic.site_rankings.clone())
                .unwrap_or_default(),
            simple_terms_text,
            tantivy_query,
            optic,
            offset: query.num_results * query.page,
            region: query.selected_region,
            top_n: query.num_results,
        })
    }

    pub fn simple_terms(&self) -> &[String] {
        &self.simple_terms_text
    }

    pub fn terms(&self) -> &[Box<Term>] {
        &self.terms
    }

    pub fn optic(&self) -> Option<&Optic> {
        self.optic.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    pub fn num_results(&self) -> usize {
        self.top_n
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn region(&self) -> Option<&Region> {
        self.region.as_ref()
    }

    pub fn site_rankings(&self) -> &SiteRankings {
        &self.site_rankings
    }

    pub fn signal_coefficients(&self) -> SignalCoefficient {
        self.optic
            .as_ref()
            .map(|optic| {
                SignalCoefficient::new(optic.rankings.iter().filter_map(|coeff| {
                    match &coeff.target {
                        RankingTarget::Signal(signal) => Signal::from_str(signal)
                            .ok()
                            .map(|signal| (signal, coeff.value)),
                    }
                }))
            })
            .unwrap_or_default()
    }
}

impl tantivy::query::Query for Query {
    fn weight(
        &self,
        enable_scoring: tantivy::query::EnableScoring,
    ) -> tantivy::Result<Box<dyn tantivy::query::Weight>> {
        self.tantivy_query.weight(enable_scoring)
    }

    fn query_terms<'a>(&'a self, visitor: &mut dyn FnMut(&'a tantivy::Term, bool)) {
        self.tantivy_query.query_terms(visitor)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        index::Index,
        rand_words,
        searcher::{LocalSearcher, SearchQuery},
        webpage::Webpage,
    };

    use super::*;

    fn empty_index() -> InvertedIndex {
        InvertedIndex::temporary().unwrap()
    }

    #[test]
    fn simple_parse() {
        let index = empty_index();
        let ctx = index.local_search_ctx();

        let query = Query::parse(
            &ctx,
            &SearchQuery {
                query: "this is a simple query the the the the the the the the the the the the the"
                    .to_string(),
                ..Default::default()
            },
            &index,
        )
        .expect("Failed to parse query");

        assert_eq!(
            query.simple_terms(),
            vec![
                "this".to_string(),
                "is".to_string(),
                "a".to_string(),
                "simple".to_string(),
                "query".to_string(),
                "the".to_string(),
                "the".to_string(),
                "the".to_string(),
                "the".to_string(),
                "the".to_string(),
                "the".to_string(),
                "the".to_string(),
                "the".to_string(),
                "the".to_string(),
                "the".to_string(),
            ]
        );
    }

    #[test]
    fn parse_trailing_leading_whitespace() {
        let index = empty_index();
        let ctx = index.local_search_ctx();

        let query = Query::parse(
            &ctx,
            &SearchQuery {
                query: "   this is a simple query   ".to_string(),
                ..Default::default()
            },
            &index,
        )
        .expect("Failed to parse query");

        assert_eq!(
            query.simple_terms(),
            vec![
                "this".to_string(),
                "is".to_string(),
                "a".to_string(),
                "simple".to_string(),
                "query".to_string(),
            ]
        );
    }

    #[test]
    fn parse_weird_characters() {
        let index = empty_index();
        let ctx = index.local_search_ctx();

        let terms = Query::parse(
            &ctx,
            &SearchQuery {
                query: "123".to_string(),
                ..Default::default()
            },
            &index,
        )
        .expect("Failed to parse query")
        .simple_terms()
        .to_vec();
        assert_eq!(terms, vec!["123".to_string()]);

        let terms = Query::parse(
            &ctx,
            &SearchQuery {
                query: "123 33".to_string(),
                ..Default::default()
            },
            &index,
        )
        .expect("Failed to parse query")
        .simple_terms()
        .to_vec();
        assert_eq!(terms, vec!["123".to_string(), "33".to_string()]);

        let terms = Query::parse(
            &ctx,
            &SearchQuery {
                query: "term! term# $".to_string(),
                ..Default::default()
            },
            &index,
        )
        .expect("Failed to parse query")
        .simple_terms()
        .to_vec();
        assert_eq!(
            terms,
            vec!["term!".to_string(), "term#".to_string(), "$".to_string()]
        );
    }

    #[test]
    fn not_query() {
        let mut index = Index::temporary().expect("Unable to open index");
        let query = SearchQuery {
            query: "test -website".to_string(),
            ..Default::default()
        };

        index
            .insert(Webpage::new(
                r#"
                        <html>
                            <head>
                                <title>Test website</title>
                            </head>
                            <body>
                                This is a test website
                            </body>
                        </html>
                    "#,
                "https://www.first.com",
            ))
            .expect("failed to insert webpage");
        index
            .insert(Webpage::new(
                r#"
                        <html>
                            <head>
                                <title>Test test</title>
                            </head>
                            <body>
                                This test page does not contain the forbidden word
                            </body>
                        </html>
                    "#,
                "https://www.second.com",
            ))
            .expect("failed to insert webpage");
        index.commit().expect("failed to commit index");
        let searcher = LocalSearcher::from(index);

        let result = searcher.search(&query).expect("Search failed");
        assert_eq!(result.num_hits, 1);
        assert_eq!(result.webpages.len(), 1);
        assert_eq!(result.webpages[0].url, "https://www.second.com");
    }

    #[test]
    fn site_query() {
        let mut index = Index::temporary().expect("Unable to open index");

        index
            .insert(Webpage::new(
                r#"
                        <html>
                            <head>
                                <title>Test website</title>
                            </head>
                            <body>
                                This is a test website
                            </body>
                        </html>
                    "#,
                "https://www.first.com",
            ))
            .expect("failed to insert webpage");
        index
            .insert(Webpage::new(
                r#"
                        <html>
                            <head>
                                <title>Test test</title>
                            </head>
                            <body>
                                This test page does not contain the forbidden word
                            </body>
                        </html>
                    "#,
                "https://www.second.com",
            ))
            .expect("failed to insert webpage");
        index.commit().expect("failed to commit index");
        let searcher = LocalSearcher::from(index);

        let query = SearchQuery {
            query: "test site:first.com".to_string(),
            ..Default::default()
        };
        let result = searcher.search(&query).expect("Search failed");
        assert_eq!(result.num_hits, 1);
        assert_eq!(result.webpages.len(), 1);
        assert_eq!(result.webpages[0].url, "https://www.first.com");

        let query = SearchQuery {
            query: "test site:www.first.com".to_string(),
            ..Default::default()
        };
        let result = searcher.search(&query).expect("Search failed");
        assert_eq!(result.num_hits, 1);
        assert_eq!(result.webpages.len(), 1);
        assert_eq!(result.webpages[0].url, "https://www.first.com");

        let query = SearchQuery {
            query: "test -site:first.com".to_string(),
            ..Default::default()
        };
        let result = searcher.search(&query).expect("Search failed");
        assert_eq!(result.num_hits, 1);
        assert_eq!(result.webpages.len(), 1);
        assert_eq!(result.webpages[0].url, "https://www.second.com");
    }

    #[test]
    fn title_query() {
        let mut index = Index::temporary().expect("Unable to open index");

        index
            .insert(Webpage::new(
                r#"
                        <html>
                            <head>
                                <title>Test website</title>
                            </head>
                            <body>
                                This is a test website
                            </body>
                        </html>
                    "#,
                "https://www.first.com",
            ))
            .expect("failed to insert webpage");
        index
            .insert(Webpage::new(
                r#"
                        <html>
                            <head>
                                <title>Test test</title>
                            </head>
                            <body>
                                This is a test website
                            </body>
                        </html>
                    "#,
                "https://www.second.com",
            ))
            .expect("failed to insert webpage");
        index.commit().expect("failed to commit index");
        let searcher = LocalSearcher::from(index);

        let query = SearchQuery {
            query: "intitle:website".to_string(),
            ..Default::default()
        };
        let result = searcher.search(&query).expect("Search failed");
        assert_eq!(result.num_hits, 1);
        assert_eq!(result.webpages.len(), 1);
        assert_eq!(result.webpages[0].url, "https://www.first.com");
    }

    #[test]
    fn url_query() {
        let mut index = Index::temporary().expect("Unable to open index");

        index
            .insert(Webpage::new(
                r#"
                        <html>
                            <head>
                                <title>Test website</title>
                            </head>
                            <body>
                                This is a test website
                            </body>
                        </html>
                    "#,
                "https://www.first.com/forum",
            ))
            .expect("failed to insert webpage");
        index
            .insert(Webpage::new(
                r#"
                        <html>
                            <head>
                                <title>Test test</title>
                            </head>
                            <body>
                                This is a test website
                            </body>
                        </html>
                    "#,
                "https://www.second.com",
            ))
            .expect("failed to insert webpage");
        index.commit().expect("failed to commit index");
        let searcher = LocalSearcher::from(index);

        let query = SearchQuery {
            query: "test inurl:forum".to_string(),
            ..Default::default()
        };
        let result = searcher.search(&query).expect("Search failed");
        assert_eq!(result.num_hits, 1);
        assert_eq!(result.webpages.len(), 1);
        assert_eq!(result.webpages[0].url, "https://www.first.com/forum");
    }

    #[test]
    fn empty_query() {
        let index = empty_index();
        let ctx = index.local_search_ctx();

        let query = Query::parse(
            &ctx,
            &SearchQuery {
                query: "".to_string(),
                ..Default::default()
            },
            &index,
        )
        .expect("failed to parse query");

        assert!(query.is_empty())
    }

    #[test]
    fn query_term_only_special_char() {
        let index = empty_index();
        let ctx = index.local_search_ctx();

        let query = Query::parse(
            &ctx,
            &SearchQuery {
                query: "&".to_string(),
                ..Default::default()
            },
            &index,
        )
        .expect("Failed to parse query");

        assert!(!query.is_empty());
    }

    #[test]
    fn site_query_split_domain() {
        let mut index = Index::temporary().expect("Unable to open index");

        index
            .insert(Webpage::new(
                &format!(
                    r#"
                        <html>
                            <head>
                                <title>Test website</title>
                            </head>
                            <body>
                                This is a test website {}
                            </body>
                        </html>
                    "#,
                    rand_words(1000)
                ),
                "https://www.the-first.com",
            ))
            .expect("failed to insert webpage");
        index
            .insert(Webpage::new(
                &format!(
                    r#"
                        <html>
                            <head>
                                <title>Test test</title>
                            </head>
                            <body>
                                This test page does not contain the forbidden word {}
                            </body>
                        </html>
                    "#,
                    rand_words(1000)
                ),
                "https://www.second.com",
            ))
            .expect("failed to insert webpage");
        index.commit().expect("failed to commit index");
        let searcher = LocalSearcher::from(index);

        let query = SearchQuery {
            query: "test site:first.com".to_string(),
            ..Default::default()
        };
        let result = searcher.search(&query).expect("Search failed");
        assert_eq!(result.num_hits, 0);
        assert_eq!(result.webpages.len(), 0);

        let query = SearchQuery {
            query: "test site:the-first.com".to_string(),
            ..Default::default()
        };
        let result = searcher.search(&query).expect("Search failed");
        assert_eq!(result.num_hits, 1);
        assert_eq!(result.webpages.len(), 1);
        assert_eq!(result.webpages[0].url, "https://www.the-first.com");

        let query = SearchQuery {
            query: "test site:www.the-first.com".to_string(),
            ..Default::default()
        };
        let result = searcher.search(&query).expect("Search failed");
        assert_eq!(result.num_hits, 1);
        assert_eq!(result.webpages.len(), 1);
        assert_eq!(result.webpages[0].url, "https://www.the-first.com");
    }

    #[test]
    fn phrase_query() {
        let mut index = Index::temporary().expect("Unable to open index");

        index
            .insert(Webpage::new(
                &format!(
                    r#"
                        <html>
                            <head>
                                <title>Test website</title>
                            </head>
                            <body>
                                This is a test website {}
                            </body>
                        </html>
                    "#,
                    rand_words(1000)
                ),
                "https://www.first.com",
            ))
            .expect("failed to insert webpage");
        index
            .insert(Webpage::new(
                &format!(
                    r#"
                        <html>
                            <head>
                                <title>Test test</title>
                            </head>
                            <body>
                                This is a bad test website {}
                            </body>
                        </html>
                    "#,
                    rand_words(1000)
                ),
                "https://www.second.com",
            ))
            .expect("failed to insert webpage");
        index.commit().expect("failed to commit index");
        let searcher = LocalSearcher::from(index);

        let query = SearchQuery {
            query: "\"Test website\"".to_string(),
            ..Default::default()
        };
        let result = searcher.search(&query).expect("Search failed");
        assert_eq!(result.num_hits, 1);
        assert_eq!(result.webpages.len(), 1);
        assert_eq!(result.webpages[0].url, "https://www.first.com");

        let query = SearchQuery {
            query: "\"Test website\" site:www.second.com".to_string(),
            ..Default::default()
        };
        let result = searcher.search(&query).expect("Search failed");
        assert_eq!(result.num_hits, 0);
        assert_eq!(result.webpages.len(), 0);
    }
}
