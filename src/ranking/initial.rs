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

use std::sync::Arc;

use crate::webpage::region::{Region, RegionCount};
use chrono::Utc;
use tantivy::collector::{ScoreSegmentTweaker, ScoreTweaker};
use tantivy::{DocId, Score, SegmentReader};

use super::SignalAggregator;

pub(crate) struct InitialScoreTweaker {
    region_count: Arc<RegionCount>,
    selected_region: Option<Region>,
    aggregator: SignalAggregator,
}

impl InitialScoreTweaker {
    pub fn new(
        region_count: Arc<RegionCount>,
        selected_region: Option<Region>,
        aggregator: SignalAggregator,
    ) -> Self {
        Self {
            region_count,
            selected_region,
            aggregator,
        }
    }
}

pub(crate) struct InitialSegmentScoreTweaker {
    aggregator: SignalAggregator,
    region_count: Arc<RegionCount>,
    current_timestamp: usize,
    selected_region: Option<Region>,
}

impl ScoreTweaker<f64> for InitialScoreTweaker {
    type Child = InitialSegmentScoreTweaker;

    fn segment_tweaker(&self, segment_reader: &SegmentReader) -> tantivy::Result<Self::Child> {
        let mut aggregator = SignalAggregator::new_like(&self.aggregator);
        aggregator.register_readers(segment_reader);

        let current_timestamp = Utc::now().timestamp() as usize;

        Ok(InitialSegmentScoreTweaker {
            aggregator,
            current_timestamp,
            selected_region: self.selected_region,
            region_count: Arc::clone(&self.region_count),
        })
    }
}

impl ScoreSegmentTweaker<f64> for InitialSegmentScoreTweaker {
    fn score(&mut self, doc: DocId, score: Score) -> f64 {
        self.aggregator.score(
            doc,
            score,
            &self.region_count,
            self.current_timestamp,
            self.selected_region,
        )
    }
}
