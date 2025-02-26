// Cuely is an open source web search engine.
// Copyright (C) 2022 Cuely ApS
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

use std::str::FromStr;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::bangs::Bangs;
use crate::entity_index::{EntityIndex, StoredEntity};
use crate::image_store::Image;
use crate::index::Index;
use crate::query::Query;
use crate::ranking::goggles;
use crate::ranking::{Ranker, SignalAggregator};
use crate::webpage::region::Region;
use crate::webpage::Url;
use crate::{inverted_index, Error, Result};

use super::{InitialSearchResult, SearchQuery, SearchResult, WebsitesResult, NUM_RESULTS_PER_PAGE};

pub struct LocalSearcher {
    index: Index,
    entity_index: Option<EntityIndex>,
    bangs: Option<Bangs>,
}

impl From<Index> for LocalSearcher {
    fn from(index: Index) -> Self {
        Self::new(index, None, None)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InitialWebsiteResult {
    pub spell_corrected_query: Option<String>,
    pub websites: inverted_index::InitialSearchResult,
    pub entity: Option<StoredEntity>,
}

impl LocalSearcher {
    pub fn new(index: Index, entity_index: Option<EntityIndex>, bangs: Option<Bangs>) -> Self {
        LocalSearcher {
            index,
            entity_index,
            bangs,
        }
    }

    pub fn search_initial(
        &self,
        query: &SearchQuery,
        de_rank_similar: bool,
    ) -> Result<InitialSearchResult> {
        let raw_query = query.original.clone();
        let goggle = query
            .goggle_program
            .as_ref()
            .and_then(|program| goggles::parse(program).ok());

        let mut parsed_query = Query::parse(
            &query.original,
            self.index.schema(),
            self.index.tokenizers(),
            goggle
                .as_ref()
                .map(|goggle| &goggle.aggregator)
                .unwrap_or(&SignalAggregator::default()),
        )?;

        if parsed_query.is_empty() {
            return Err(Error::EmptyQuery);
        }

        if let Some(bangs) = self.bangs.as_ref() {
            if let Some(bang) = bangs.get(&parsed_query) {
                return Ok(InitialSearchResult::Bang(bang));
            }
        }

        let mut goggles = Vec::new();

        if let Some(goggle) = &goggle {
            goggles.push(goggle.clone());
        }

        if let Some(site_rankings) = &query.site_rankings {
            goggles.push(site_rankings.clone().into_goggle())
        }

        parsed_query.set_goggles(&goggles, &self.index.schema());

        let mut ranker = Ranker::new(
            self.index.region_count.clone(),
            goggle.map(|goggle| goggle.aggregator).unwrap_or_default(),
            self.index.inverted_index.fastfield_cache(),
        );

        if let Some(skip_pages) = query.skip_pages {
            ranker = ranker.with_offset(NUM_RESULTS_PER_PAGE * skip_pages);
        }

        if let Some(region) = query.selected_region {
            if region != Region::All {
                ranker = ranker.with_region(region);
            }
        }

        ranker = ranker.with_max_docs(10_000_000, self.index.num_segments());
        ranker.de_rank_similar(de_rank_similar);

        let webpages = self
            .index
            .search_initial(&parsed_query, ranker.collector())?;
        let correction = self.index.spell_correction(&parsed_query.simple_terms());

        let entity = self
            .entity_index
            .as_ref()
            .and_then(|index| index.search(&raw_query));

        Ok(InitialSearchResult::Websites(InitialWebsiteResult {
            spell_corrected_query: correction,
            websites: webpages,
            entity,
        }))
    }

    pub fn retrieve_websites(
        &self,
        websites: &[inverted_index::WebsitePointer],
        query: &str,
    ) -> Result<Vec<inverted_index::RetrievedWebpage>> {
        let query = Query::parse(
            query,
            self.index.schema(),
            self.index.tokenizers(),
            &SignalAggregator::default(),
        )?;

        if query.is_empty() {
            return Err(Error::EmptyQuery);
        }

        self.index.retrieve_websites(websites, &query)
    }

    pub fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        let start = Instant::now();

        let query_text = query.original.clone();
        let initial_result = self.search_initial(query, true)?;

        match initial_result {
            InitialSearchResult::Websites(search_result) => {
                let retrieved_sites =
                    self.retrieve_websites(&search_result.websites.top_websites, &query_text)?;

                Ok(SearchResult::Websites(WebsitesResult {
                    spell_corrected_query: search_result.spell_corrected_query,
                    webpages: inverted_index::SearchResult {
                        num_docs: search_result.websites.num_websites,
                        documents: retrieved_sites,
                    },
                    entity: search_result.entity,
                    search_duration_ms: start.elapsed().as_millis(),
                }))
            }
            InitialSearchResult::Bang(bang) => Ok(SearchResult::Bang(bang)),
        }
    }

    pub fn favicon(&self, site: &Url) -> Option<Image> {
        self.index.retrieve_favicon(site)
    }

    pub fn primary_image(&self, uuid: String) -> Option<Image> {
        if let Ok(uuid) = Uuid::from_str(uuid.as_str()) {
            return self.index.retrieve_primary_image(&uuid);
        }
        None
    }

    pub fn entity_image(&self, entity: String) -> Option<Image> {
        self.entity_index
            .as_ref()
            .and_then(|index| index.retrieve_image(&entity))
    }

    pub fn attribute_occurrence(&self, attribute: &String) -> Option<u32> {
        self.entity_index
            .as_ref()
            .and_then(|index| index.get_attribute_occurrence(attribute))
    }
}
impl SearchResult {
    #[cfg(test)]
    pub fn into_websites(self) -> Option<WebsitesResult> {
        if let SearchResult::Websites(res) = self {
            Some(res)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::webpage::{Html, Webpage};

    use super::*;

    #[test]
    fn offset_page() {
        const NUM_PAGES: usize = 10;
        const NUM_WEBSITES: usize = NUM_PAGES * NUM_RESULTS_PER_PAGE;

        let mut index = Index::temporary().expect("Unable to open index");

        for i in 0..NUM_WEBSITES {
            index
                .insert(Webpage {
                    html: Html::parse(
                        r#"
            <html>
                <head>
                    <title>Example website</title>
                </head>
                <body>
                    test
                </body>
            </html>
            "#,
                        &format!("https://www.{i}.com"),
                    ),
                    backlinks: vec![],
                    host_centrality: (NUM_WEBSITES - i) as f64,
                    fetch_time_ms: 500,
                    page_centrality: 0.0,
                    pre_computed_score: 0.0,
                    primary_image: None,
                })
                .expect("failed to insert webpage");
        }

        index.commit().unwrap();

        let searcher = LocalSearcher::new(index, None, None);

        for p in 0..NUM_PAGES {
            let urls: Vec<_> = searcher
                .search(&SearchQuery {
                    original: "test".to_string(),
                    selected_region: None,
                    goggle_program: None,
                    skip_pages: Some(p),
                    site_rankings: None,
                })
                .unwrap()
                .into_websites()
                .unwrap()
                .webpages
                .documents
                .into_iter()
                .map(|page| page.url)
                .collect();

            assert!(!urls.is_empty());

            for (i, url) in urls.into_iter().enumerate() {
                assert_eq!(
                    url,
                    format!("https://www.{}.com", i + (p * NUM_RESULTS_PER_PAGE))
                )
            }
        }
    }
}
