// Stract is an open source web search engine.
// Copyright (C) 2024 Stract ApS
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

//! WASM Bindings for Client Side JavaScript
//!
//! To be packaged with wasm-pack + vite and served to the browser

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tsify::Tsify;
use wasm_bindgen::prelude::*;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to serialize")]
    Serialization(#[from] serde_wasm_bindgen::Error),

    #[error("Optics error: {0}")]
    OpticParse(#[from] optics::Error),

    #[error("Json serialization error: {0}")]
    SerdeJson(#[from] serde_json::Error),
}

impl From<Error> for JsValue {
    fn from(val: Error) -> Self {
        JsValue::from_str(&format!("{val:?}"))
    }
}

#[derive(Tsify, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct SiteRankings {
    pub sites: HashMap<String, Ranking>,
}

#[derive(Clone, Tsify, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Ranking {
    Liked,
    Disliked,
    Blocked,
}

/// Takes the contents of a `.optic` file and converts it to a `Result` containing
/// either an error or [`SiteRankings`]
#[wasm_bindgen(js_name = parsePreferenceOptic)]
pub fn site_rankings_from_optic(optic_contents: String) -> Result<SiteRankings, Error> {
    let optic = optics::Optic::parse(&optic_contents)?;
    let sites = [
        (Ranking::Liked, &optic.host_rankings.liked),
        (Ranking::Disliked, &optic.host_rankings.disliked),
        (Ranking::Blocked, &optic.host_rankings.blocked),
    ]
    .iter()
    .flat_map(|(ranking, sites)| {
        sites
            .iter()
            .map(move |site| (site.clone(), ranking.clone()))
    })
    .collect();
    Ok(SiteRankings { sites })
}
